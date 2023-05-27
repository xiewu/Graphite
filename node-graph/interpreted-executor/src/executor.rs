use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::pin::Pin;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use dyn_any::StaticType;
use graph_craft::document::value::{TaggedValue, UpcastNode};
use graph_craft::document::NodeId;
use graph_craft::proto::{AnyNodeConstructor, ConstructionArgs, DynFuture, NodeConstructor, ProtoNetwork, ProtoNode, TypeErasedNode, TypingContext};
use graph_craft::Type;
use graphene_std::any::{Any, TypeErasedPinned, TypeErasedPinnedRef};

use crate::node_registry;

#[derive(Clone)]
pub struct DynamicExecutor<'n> {
	output: NodeId,
	tree: BorrowTree,
	typing_context: TypingContext<'n>,
	// This allows us to keep the nodes around for one more frame which is used for introspection
	orphaned_nodes: Vec<NodeId>,
}

impl<'n> Default for DynamicExecutor<'n> {
	fn default() -> Self {
		Self {
			output: Default::default(),
			tree: Default::default(),
			typing_context: TypingContext::new(&node_registry::NODE_REGISTRY),
			orphaned_nodes: Vec::new(),
		}
	}
}

impl<'n> DynamicExecutor<'n> {
	pub async fn update(&'n mut self, proto_network: ProtoNetwork) -> Result<(), String> {
		self.output = proto_network.output;
		self.typing_context.update(&proto_network)?;
		trace!("setting output to {}", self.output);
		let mut orphans = self.tree.update(proto_network, &self.typing_context).await?;
		core::mem::swap(&mut self.orphaned_nodes, &mut orphans);
		for node_id in orphans {
			if self.orphaned_nodes.contains(&node_id) {
				self.tree.free_node(node_id)
			}
		}
		Ok(())
	}

	pub fn introspect(&self, node_path: &[NodeId]) -> Option<Option<Arc<dyn std::any::Any>>> {
		self.tree.introspect(node_path)
	}

	pub fn input_type(&self) -> Option<Type> {
		self.typing_context.type_of(self.output).map(|node_io| node_io.input.clone())
	}

	pub fn output_type(&self) -> Option<Type> {
		self.typing_context.type_of(self.output).map(|node_io| node_io.output.clone())
	}
}

#[ouroboros::self_referencing]
pub struct NodeContainer {
	// the dependencies are only kept to ensure that the nodes are not dropped while still in use
	_dependencies: Vec<Arc<NodeContainer>>,
	#[borrows(_dependencies)]
	#[covariant]
	pub node: TypeErasedPinned<'this, 'this>,
}

impl<'n: 'i, 'i> core::fmt::Debug for NodeContainer {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("NodeContainer").finish()
	}
}

#[derive(Default, Debug, Clone)]
pub struct BorrowTree {
	nodes: HashMap<NodeId, Arc<NodeContainer>>,
	source_map: HashMap<Vec<NodeId>, NodeId>,
}

impl<'n: 'i, 'i> BorrowTree {
	pub async fn new<'a: 'b, 'b>(proto_network: ProtoNetwork, typing_context: &'a TypingContext<'a>) -> Result<BorrowTree, String> {
		let mut nodes = BorrowTree::default();
		for (id, node) in proto_network.nodes {
			nodes.push_node(id, node, typing_context).await?
		}
		Ok(nodes)
	}

	/// Pushes new nodes into the tree and return orphaned nodes
	pub async fn update(&mut self, proto_network: ProtoNetwork, typing_context: &'n TypingContext<'n>) -> Result<Vec<NodeId>, String> {
		let mut old_nodes: HashSet<_> = self.nodes.keys().copied().collect();
		for (id, node) in proto_network.nodes {
			if !self.nodes.contains_key(&id) {
				self.push_node(id, node, typing_context).await?;
			} else {
				let Some(node_container) = self.nodes.get_mut(&id) else { continue };
				// TODO: decide when we want to reset the node
				//unsafe { node_container.node.reset() };
			}
			old_nodes.remove(&id);
		}
		self.source_map.retain(|_, nid| !old_nodes.contains(nid));
		Ok(old_nodes.into_iter().collect())
	}

	fn node_deps(&self, nodes: &[NodeId]) -> Vec<Arc<NodeContainer>> {
		nodes.iter().map(|node| self.nodes.get(node).unwrap().clone()).collect()
	}

	fn store_node(&mut self, node: Arc<NodeContainer>, id: NodeId) -> Arc<NodeContainer> {
		self.nodes.insert(id, node.clone());
		node
	}

	pub fn introspect(&self, node_path: &[NodeId]) -> Option<Option<Arc<dyn std::any::Any>>> {
		let id = self.source_map.get(node_path)?;
		let node = self.nodes.get(id)?;
		Some(node.borrow_node().serialize())
	}

	pub fn get(&self, id: NodeId) -> Option<Arc<NodeContainer>> {
		self.nodes.get(&id).cloned()
	}

	/*
	pub async fn eval<'a, I: StaticType + 'a + Send + Sync, O: StaticType + Send + Sync + 'a>(&'n self, id: NodeId, input: I) -> Option<O>
	where
		'i: 'a,
		'n: 'a,
	{
		let output = self.eval_any(id, Box::new(input)).await?;
		dyn_any::downcast::<O>(output).ok().map(|o| *o)
	}*/

	pub async fn eval_any<'a>(&'n self, node: &'i NodeContainer, input: Any<'i>) -> Option<Any<'i>>
	where
		'i: 'a,
		'n: 'a,
	{
		let output = node.borrow_node().eval(Box::new(input));
		Some(output.await)
	}

	pub fn free_node(&mut self, id: NodeId) {
		self.nodes.remove(&id);
	}

	pub async fn push_node(&mut self, id: NodeId, proto_node: ProtoNode, typing_context: &'n TypingContext<'n>) -> Result<(), String> {
		let ProtoNode {
			construction_args,
			identifier,
			document_node_path,
			..
		} = proto_node;
		self.source_map.insert(document_node_path, id);

		match construction_args {
			ConstructionArgs::Value(value) => {
				let node = NodeContainer::new(vec![], move |_| {
					let upcasted = UpcastNode::new(value);
					let node = Box::pin(upcasted) as TypeErasedPinned<'_, '_>;
					node
				});
				self.store_node(Arc::new(node.into()), id);
			}
			ConstructionArgs::Nodes(ids) => {
				let ids: Vec<_> = ids.iter().map(|(id, _)| *id).collect();
				let construction_nodes = self.node_deps(&ids);
				let constructor = typing_context.constructor(id).ok_or(format!("No constructor found for node {:?}", identifier))?;
				let node = NodeContainer::new_async(construction_nodes, move |deps| {
					let deps = deps.iter().map(|dep| dep.borrow_node().as_ref()).collect::<Vec<_>>();
					constructor(deps)
				})
				.await;
				self.store_node(Arc::new(node.into()), id);
			}
		};
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use graph_craft::document::value::TaggedValue;

	use super::*;

	#[tokio::test]
	async fn push_node() {
		let mut tree = BorrowTree::default();
		let val_1_protonode = ProtoNode::value(ConstructionArgs::Value(TaggedValue::U32(2u32)), vec![]);
		tree.push_node(0, val_1_protonode, &TypingContext::default()).await.unwrap();
		let _node = tree.get(0).unwrap();
		assert_eq!(tree.eval(0, ()).await, Some(2u32));
	}
}
