use dyn_any::StaticType;

use graph_craft::document::value::TaggedValue;
use graph_craft::document::value::UpcastNode;
use graph_craft::document::NodeId;
use graph_craft::proto::{ConstructionArgs, ProtoNetwork, ProtoNode, TypingContext};

use graphene_std::any::TypeErasedCell;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Clone)]
pub struct NodeContainer<'n> {
	pub node: TypeErasedCell<'n>,
}

impl<'a> core::fmt::Debug for NodeContainer<'a> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("NodeContainer").finish()
	}
}

impl<'a> NodeContainer<'a> {
	pub fn new(node: TypeErasedCell<'a>) -> Self {
		Self { node }
	}
}

#[derive(Default, Debug, Clone)]
pub struct BorrowTree {
	pub(crate) nodes: HashMap<NodeId, NodeContainer<'static>>,
	pub(crate) source_map: HashMap<Vec<NodeId>, NodeId>,
}

impl BorrowTree {
	pub async fn new(proto_network: ProtoNetwork, typing_context: &TypingContext) -> Result<Self, String> {
		let mut nodes = BorrowTree::default();
		for (id, node) in proto_network.nodes {
			nodes.push_node(id, node, typing_context).await?
		}
		Ok(nodes)
	}

	/// Pushes new nodes into the tree and return orphaned nodes
	pub async fn update(&mut self, proto_network: ProtoNetwork, typing_context: &TypingContext) -> Result<Vec<NodeId>, String> {
		let mut old_nodes: HashSet<_> = self.nodes.keys().copied().collect();
		for (id, node) in proto_network.nodes {
			if !self.nodes.contains_key(&id) {
				self.push_node(id, node, typing_context).await?;
			} else {
				let Some(node_container) = self.nodes.get_mut(&id) else { continue };
				node_container.node.reset();
			}
			old_nodes.remove(&id);
		}
		self.source_map.retain(|_, nid| !old_nodes.contains(nid));
		Ok(old_nodes.into_iter().collect())
	}

	pub(crate) fn node_deps(&self, nodes: &[NodeId]) -> Vec<NodeContainer<'static>> {
		nodes.iter().map(|node| self.nodes.get(node).unwrap().clone()).collect()
	}

	pub(crate) fn store_node(&mut self, node: NodeContainer<'static>, id: NodeId) -> NodeContainer<'static> {
		self.nodes.insert(id, node.clone());
		node
	}

	pub fn introspect(&self, node_path: &[NodeId]) -> Option<Option<Arc<dyn std::any::Any>>> {
		let id = self.source_map.get(node_path)?;
		let node = self.nodes.get(id)?;
		Some(node.node.serialize())
	}

	pub fn get(&self, id: NodeId) -> Option<NodeContainer<'static>> {
		self.nodes.get(&id).cloned()
	}

	pub async fn eval<'i, I: StaticType + 'i, O: StaticType + 'i>(&'i self, id: NodeId, input: I) -> Option<O> {
		let node = self.nodes.get(&id).cloned()?;
		let output = node.node.eval(Box::new(input));
		dyn_any::downcast::<O>(output.await).ok().map(|o| *o)
	}
	pub async fn eval_tagged_value<'i, I: StaticType + 'i>(&'i self, id: NodeId, input: I) -> Result<TaggedValue, String> {
		let node = self.nodes.get(&id).cloned().ok_or_else(|| "Output node not found in executor")?;
		let output = node.node.eval(Box::new(input));
		TaggedValue::try_from_any(output.await)
	}

	pub fn free_node(&mut self, id: NodeId) {
		self.nodes.remove(&id);
	}

	pub async fn push_node(&mut self, id: NodeId, proto_node: ProtoNode, typing_context: &TypingContext) -> Result<(), String> {
		let ProtoNode {
			construction_args,
			identifier,
			document_node_path,
			..
		} = proto_node;
		self.source_map.insert(document_node_path, id);

		match construction_args {
			ConstructionArgs::Value(value) => {
				let upcasted = UpcastNode::new(value);
				let node = Arc::new(upcasted) as TypeErasedCell<'_>;
				let node = NodeContainer { node };
				self.store_node(node.into(), id);
			}
			ConstructionArgs::Inline(_) => unimplemented!("Inline nodes are not supported yet"),
			ConstructionArgs::Nodes(ids) => {
				let ids: Vec<_> = ids.iter().map(|(id, _)| *id).collect();
				let construction_nodes = self.node_deps(&ids);
				let constructor = typing_context.constructor(id).ok_or(format!("No constructor found for node {:?}", identifier))?;
				let node = constructor(construction_nodes).await;
				let node = NodeContainer { node };
				self.store_node(node.into(), id);
			}
		};
		Ok(())
	}
}
