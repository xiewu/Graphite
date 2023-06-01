use std::error::Error;
use std::sync::Arc;

use dyn_any::StaticType;
use graph_craft::compiler::Executor;
use graph_craft::document::value::TaggedValue;
use graph_craft::document::NodeId;
use graph_craft::proto::{DynFuture, ProtoNetwork, TypingContext};
use graph_craft::Type;

use crate::node_registry;

mod borrow_tree;

#[derive(Clone)]
pub struct DynamicExecutor {
	output: NodeId,
	tree: borrow_tree::BorrowTree,
	typing_context: TypingContext,
	// This allows us to keep the nodes around for one more frame which is used for introspection
	orphaned_nodes: Vec<NodeId>,
}

impl Default for DynamicExecutor {
	fn default() -> Self {
		Self {
			output: Default::default(),
			tree: Default::default(),
			typing_context: TypingContext::new(&node_registry::NODE_REGISTRY),
			orphaned_nodes: Vec::new(),
		}
	}
}

impl DynamicExecutor {
	pub async fn new(proto_network: ProtoNetwork) -> Result<Self, String> {
		let mut typing_context = TypingContext::new(&node_registry::NODE_REGISTRY);
		typing_context.update(&proto_network)?;
		let output = proto_network.output;
		let tree = borrow_tree::BorrowTree::new(proto_network, &typing_context).await?;

		Ok(Self {
			tree,
			output,
			typing_context,
			orphaned_nodes: Vec::new(),
		})
	}

	pub async fn update(&mut self, proto_network: ProtoNetwork) -> Result<(), String> {
		self.output = proto_network.output;
		self.typing_context.update(&proto_network)?;
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

impl<'a, I: StaticType + 'a> Executor<I, TaggedValue> for &'a DynamicExecutor {
	fn execute(&self, input: I) -> DynFuture<Result<TaggedValue, Box<dyn Error>>> {
		Box::pin(async move { self.tree.eval_tagged_value(self.output, input).await.map_err(|e| e.into()) })
	}
}

#[cfg(test)]
mod test {
	use graph_craft::document::value::TaggedValue;

	use super::*;

	#[test]
	fn push_node_sync() {
		let mut tree = borrow_tree::BorrowTree::default();
		let val_1_protonode = ProtoNode::value(ConstructionArgs::Value(TaggedValue::U32(2u32)), vec![]);
		let context = TypingContext::default();
		let future = tree.push_node(0, val_1_protonode, &context); //.await.unwrap();
		futures::executor::block_on(future).unwrap();
		let _node = tree.get(0).unwrap();
		let result = futures::executor::block_on(tree.eval(0, ()));
		assert_eq!(result, Some(2u32));
	}
}
