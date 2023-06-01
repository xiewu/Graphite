use dyn_any::{StaticType, StaticTypeSized};
use gpu_executor::ShaderInput;
pub use graph_craft::proto::{Any, TypeErasedCell, TypeErasedNode};
use graph_craft::proto::{AnySlotInput, DynFuture, FutureAny, SlotInput};
use graphene_core::NodeIO;
pub use graphene_core::{generic, ops, Node};
use std::{marker::PhantomData, sync::Arc};

pub struct DynAnyNode<I, O, Node> {
	node: Node,
	_i: PhantomData<I>,
	_o: PhantomData<O>,
}

impl<'input, _I: 'input + StaticType, _O: 'input + StaticType, N: 'input, S0: 'input> Node<'input, Any<'input>> for DynAnyNode<_I, _O, S0>
where
	N: for<'any_input> Node<'any_input, _I, Output = DynFuture<'any_input, _O>>,
	S0: for<'any_input> Node<'any_input, (), Output = &'any_input N>,
{
	type Output = FutureAny<'input>;
	#[inline]
	fn eval(&'input self, input: Any<'input>) -> Self::Output {
		let node = self.node.eval(());
		let node_name = core::any::type_name::<N>();
		let input: Box<_I> = dyn_any::downcast(input).unwrap_or_else(|e| panic!("DynAnyNode Input, {0} in:\n{1}", e, node_name));
		let output = async move {
			let result = node.eval(*input).await;
			Box::new(result) as Any<'input>
		};
		Box::pin(output)
	}

	fn reset(&self) {
		self.node.reset();
	}

	fn serialize(&self) -> Option<std::sync::Arc<dyn core::any::Any>> {
		self.node.eval(()).serialize()
	}
}
impl<'input, _I: StaticType, _O: StaticType, N, S0: 'input> DynAnyNode<_I, _O, S0>
where
	S0: for<'any_input> Node<'any_input, (), Output = &'any_input N>,
{
	pub const fn new(node: S0) -> Self {
		Self {
			node,
			_i: core::marker::PhantomData,
			_o: core::marker::PhantomData,
		}
	}
}

pub struct DynAnyRefNode<I, O, Node> {
	node: Node,
	_i: PhantomData<(I, O)>,
}
impl<'input, _I: 'input + StaticType, _O: 'input + StaticType, N: 'input> Node<'input, Any<'input>> for DynAnyRefNode<_I, _O, N>
where
	N: for<'any_input> Node<'any_input, _I, Output = &'any_input _O>,
{
	type Output = FutureAny<'input>;
	fn eval(&'input self, input: Any<'input>) -> Self::Output {
		let node_name = core::any::type_name::<N>();
		let input: Box<_I> = dyn_any::downcast(input).unwrap_or_else(|e| panic!("DynAnyRefNode Input, {e} in:\n{node_name}"));
		let result = self.node.eval(*input);
		let output = async move { Box::new(result) as Any<'input> };
		Box::pin(output)
	}
	fn reset(&self) {
		self.node.reset();
	}
}

impl<_I, _O, S0> DynAnyRefNode<_I, _O, S0> {
	pub const fn new(node: S0) -> Self {
		Self { node, _i: core::marker::PhantomData }
	}
}
pub struct DynAnyInRefNode<I, O, Node> {
	node: Node,
	_i: PhantomData<(I, O)>,
}
impl<'input, _I: 'input + StaticType, _O: 'input + StaticType, N: 'input> Node<'input, Any<'input>> for DynAnyInRefNode<_I, _O, N>
where
	N: for<'any_input> Node<'any_input, &'any_input _I, Output = DynFuture<'any_input, _O>>,
{
	type Output = FutureAny<'input>;
	fn eval(&'input self, input: Any<'input>) -> Self::Output {
		{
			let node_name = core::any::type_name::<N>();
			let input: Box<&_I> = dyn_any::downcast(input).unwrap_or_else(|e| panic!("DynAnyInRefNode Input, {e} in:\n{node_name}"));
			let result = self.node.eval(*input);
			Box::pin(async move { Box::new(result.await) as Any<'_> })
		}
	}
}
impl<_I, _O, S0> DynAnyInRefNode<_I, _O, S0> {
	pub const fn new(node: S0) -> Self {
		Self { node, _i: core::marker::PhantomData }
	}
}

pub struct FutureWrapperNode<Node> {
	node: Node,
}

impl<'i, T: 'i, N: Node<'i, T>> Node<'i, T> for FutureWrapperNode<N>
where
	N: Node<'i, T>,
{
	type Output = DynFuture<'i, N::Output>;
	fn eval(&'i self, input: T) -> Self::Output {
		Box::pin(async move { self.node.eval(input) })
	}
	fn reset(&self) {
		self.node.reset();
	}
}

impl<'i, N> FutureWrapperNode<N> {
	pub const fn new(node: N) -> Self {
		Self { node }
	}
}

pub trait IntoTypeErasedNode<'n> {
	fn into_type_erased(self) -> TypeErasedCell<'n>;
}

impl<'n, N: 'n> IntoTypeErasedNode<'n> for N
where
	N: for<'i> NodeIO<'i, AnySlotInput<'i>, Output = FutureAny<'i>> + 'n,
{
	fn into_type_erased(self) -> TypeErasedCell<'n> {
		Box::new(self)
	}
}

pub struct DowncastNode<O, Node> {
	node: Node,
	_o: PhantomData<O>,
}
impl<N: Clone, O: StaticType> Clone for DowncastNode<O, N> {
	fn clone(&self) -> Self {
		Self { node: self.node.clone(), _o: self._o }
	}
}
impl<N: Copy, O: StaticType> Copy for DowncastNode<O, N> {}

/// Boxes the input and downcasts the output.
/// Wraps around a node taking Box<dyn DynAny> and returning Box<dyn DynAny>
pub struct DowncastBothNode<'n, I, O> {
	node: TypeErasedCell<'n>,
	_i: PhantomData<I>,
	_o: PhantomData<O>,
}
impl<'n: 'input, 'i: 'input, 't, 'input, O: 'input + StaticType, I: 'input + StaticTypeSized> Node<'input, SlotInput<'i, I>> for DowncastBothNode<'n, I, O> {
	type Output = DynFuture<'input, O>;
	#[inline]
	fn eval(&'input self, input: SlotInput<'input, I>) -> Self::Output {
		{
			let node_name = self.node.node_name();
			let slotmap = input.1;
			let box_input = Box::new(input);
			let future = self.node.eval(SlotInput(box_input, slotmap));
			Box::pin(async move {
				let out = dyn_any::downcast(future.await).unwrap_or_else(|e| panic!("DowncastBothNode Input {e} in: \n{node_name}"));
				*out
			})
		}
	}
}
impl<'n, I, O> DowncastBothNode<'n, I, O> {
	pub const fn new(node: TypeErasedCell<'n>) -> Self {
		Self {
			node,
			_i: core::marker::PhantomData,
			_o: core::marker::PhantomData,
		}
	}
}
/// Boxes the input and downcasts the output.
/// Wraps around a node taking Box<dyn DynAny> and returning Box<dyn DynAny>
pub struct DowncastBothRefNode<'a, I, O> {
	node: TypeErasedCell<'a>,
	_i: PhantomData<(I, O)>,
}
impl<'n: 'input, 'input, O: 'input + StaticType, I: 'input + StaticTypeSized> Node<'input, SlotInput<'input, I>> for DowncastBothRefNode<'n, I, O> {
	type Output = DynFuture<'input, &'input O>;
	#[inline]
	fn eval(&'input self, input: SlotInput<'input, I>) -> Self::Output {
		{
			let slotmap = input.1;
			let node_name = self.node.node_name();
			Box::pin(async move {
				let box_input = Box::new(input);
				let out: Box<&_> = dyn_any::downcast::<&O>(self.node.eval(SlotInput(box_input, slotmap)).await).unwrap_or_else(|e| panic!("DowncastBothRefNode Input {e}"));
				*out
			})
		}
	}
}
impl<'n, I, O> DowncastBothRefNode<'n, I, O> {
	pub const fn new(node: TypeErasedCell<'n>) -> Self {
		Self { node, _i: core::marker::PhantomData }
	}
}

pub struct ComposeTypeErased<'a> {
	first: TypeErasedCell<'a>,
	second: TypeErasedCell<'a>,
}

impl<'i, 'a, 't> Node<'i, AnySlotInput<'i>> for ComposeTypeErased<'i> {
	type Output = DynFuture<'i, Any<'i>>;
	fn eval(&'i self, input: AnySlotInput<'i>) -> Self::Output {
		let slotmap = input.1;
		Box::pin(async move {
			let arg = self.first.eval(input).await;
			self.second.eval(SlotInput(arg, slotmap)).await
		})
	}
}

impl<'a> ComposeTypeErased<'a> {
	pub const fn new(first: TypeErasedCell<'a>, second: TypeErasedCell<'a>) -> Self {
		ComposeTypeErased { first, second }
	}
}

pub fn input_node<'n, O: StaticType>(n: TypeErasedCell<'n>) -> DowncastBothNode<'n, (), O> {
	DowncastBothNode::new(n)
}

pub struct PanicNode<I, O>(PhantomData<I>, PhantomData<O>);

impl<'i, I: 'i, O: 'i> Node<'i, I> for PanicNode<I, O> {
	type Output = O;
	fn eval(&'i self, _: I) -> Self::Output {
		unimplemented!("This node should never be evaluated")
	}
}

impl<I, O> PanicNode<I, O> {
	pub const fn new() -> Self {
		Self(PhantomData, PhantomData)
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use graphene_core::{ops::AddNode, ops::IdNode, value::ValueNode};

	#[test]
	#[should_panic]
	pub fn dyn_input_invalid_eval_panic() {
		//let add = DynAnyNode::new(AddNode::new()).into_type_erased();
		//add.eval(Box::new(&("32", 32u32)));
		let dyn_any = DynAnyNode::<(u32, u32), u32, _>::new(ValueNode::new(FutureWrapperNode { node: AddNode::new() }));
		let type_erased = Box::pin(dyn_any) as TypeErasedCell;
		let _ref_type_erased = type_erased.as_ref();
		//let type_erased = Box::pin(dyn_any) as TypeErasedCell<'_>;
		type_erased.eval(Box::new(&("32", 32u32)));
	}

	#[test]
	pub fn dyn_input_invalid_eval_panic_() {
		//let add = DynAnyNode::new(AddNode::new()).into_type_erased();
		//add.eval(Box::new(&("32", 32u32)));
		let dyn_any = DynAnyNode::<(u32, u32), u32, _>::new(ValueNode::new(FutureWrapperNode { node: AddNode::new() }));
		let type_erased = Box::pin(dyn_any) as TypeErasedCell<'_>;
		type_erased.eval(Box::new((4u32, 2u32)));
		let id_node = FutureWrapperNode::new(IdNode::new());
		let type_erased_id = Box::pin(id_node) as TypeErasedCell;
		let type_erased = ComposeTypeErased::new(type_erased.as_ref(), type_erased_id.as_ref());
		type_erased.eval(Box::new((4u32, 2u32)));
		//let downcast: DowncastBothNode<(u32, u32), u32> = DowncastBothNode::new(type_erased.as_ref());
		//downcast.eval((4u32, 2u32));
	}

	// TODO: Fix this test
	/*
	#[test]
	pub fn dyn_input_storage_composition() {
		// todo readd test
		let node = <graphene_core::ops::IdNode>::new();
		let any: DynAnyNode<Any<'_>, Any<'_>, _> = DynAnyNode::new(ValueNode::new(node));
		any.into_type_erased();
	}
	*/
}
