#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg_attr(feature = "log", macro_use)]
#[cfg(feature = "log")]
extern crate log;

pub mod consts;
pub mod generic;
pub mod ops;
pub mod structural;
#[cfg(feature = "std")]
pub mod text;
#[cfg(feature = "std")]
pub mod uuid;
pub mod value;

#[cfg(feature = "gpu")]
pub mod gpu;

pub mod memo;
pub mod storage;

pub mod raster;
#[cfg(feature = "alloc")]
pub mod transform;

#[cfg(feature = "alloc")]
mod graphic_element;
#[cfg(feature = "alloc")]
pub use graphic_element::*;
#[cfg(feature = "alloc")]
pub mod vector;

pub mod application_io;

pub mod quantization;

use core::any::TypeId;
pub use raster::Color;

// pub trait Node: for<'n> NodeIO<'n> {
pub trait Node<'i, Input: 'i>: 'i {
	type Output: 'i;
	fn eval(&'i self, input: Input) -> Self::Output;
	fn reset(&self) {}
	#[cfg(feature = "std")]
	fn serialize(&self) -> Option<std::sync::Arc<dyn core::any::Any>> {
		log::warn!("Node::serialize not implemented for {}", core::any::type_name::<Self>());
		None
	}
}

#[cfg(feature = "alloc")]
mod types;
#[cfg(feature = "alloc")]
pub use types::*;

use dyn_any::StaticTypeSized;
pub trait NodeIO<'i, Input: 'i>: 'i + Node<'i, Input>
where
	Self::Output: 'i + StaticTypeSized,
	Input: 'i + StaticTypeSized,
{
	fn node_name(&self) -> &'static str {
		core::any::type_name::<Self>()
	}

	fn input_type(&self) -> TypeId {
		TypeId::of::<Input::Static>()
	}
	fn input_type_name(&self) -> &'static str {
		core::any::type_name::<Input>()
	}
	fn output_type(&self) -> core::any::TypeId {
		TypeId::of::<<Self::Output as StaticTypeSized>::Static>()
	}
	fn output_type_name(&self) -> &'static str {
		core::any::type_name::<Self::Output>()
	}
	#[cfg(feature = "alloc")]
	fn to_node_io(&self, parameters: Vec<Type>) -> NodeIOTypes {
		NodeIOTypes {
			input: concrete!(<Input as StaticTypeSized>::Static),
			output: concrete!(<Self::Output as StaticTypeSized>::Static),
			parameters,
		}
	}
}

impl<'i, N: Node<'i, I>, I> NodeIO<'i, I> for N
where
	N::Output: 'i + StaticTypeSized,
	I: 'i + StaticTypeSized,
{
}

use ghost_cell::{GhostCell, GhostToken};
impl<'i, 'brand, I: 'i, N: Node<'i, I>> Node<'i, (I, &'i GhostToken<'brand>)> for GhostCell<'brand, N> {
	type Output = N::Output;

	fn eval(&'i self, input: (I, &'i GhostToken<'brand>)) -> Self::Output {
		self.borrow(input.1).eval(input.0)
	}
}
#[cfg(feature = "alloc")]
pub mod dyn_exec {
	use super::{Node, NodeIO};
	use alloc::boxed::Box;
	use core::pin::Pin;
	use dyn_any::{DynAny, StaticType, StaticTypeSized};
	use slotmap::{DefaultKey, SlotMap};

	pub type DynFuture<'n, T> = Pin<Box<dyn core::future::Future<Output = T> + 'n>>;
	pub type Any<'n> = Box<dyn DynAny<'n> + 'n>;
	pub type FutureAny<'n> = DynFuture<'n, Any<'n>>;
	#[derive(Clone, Copy)]
	pub struct SlotInput<'i, T>(pub T, pub &'i SlotMap<DefaultKey, Box<TypeErasedNode<'i>>>);
	impl<'i, T> SlotInput<'i, T> {
		pub fn new(&self, arg: T) -> SlotInput<'i, T> {
			SlotInput(arg, self.1)
		}
	}
	unsafe impl<'i, T: StaticTypeSized> StaticType for SlotInput<'i, T> {
		type Static = SlotInput<'static, T::Static>;
	}
	pub type AnySlotInput<'i> = SlotInput<'i, Any<'i>>;
	//pub type SlotInput<'i> = (Any<'i>, &'i SlotMap<DefaultKey, Box<TypeErasedNode>>);
	pub type TypeErasedNode<'n> = dyn for<'i> NodeIO<'i, AnySlotInput<'i>, Output = FutureAny<'i>> + 'n;

	impl<'i> Node<'i, AnySlotInput<'i>> for DefaultKey {
		type Output = FutureAny<'i>;

		fn eval(&'i self, input: AnySlotInput<'i>) -> Self::Output {
			input.1[*self].eval(input)
		}
	}
}

#[cfg(feature = "alloc")]
impl<'i, 's: 'i, I: 'i, O: 'i, N: Node<'i, I, Output = O>> Node<'i, I> for alloc::sync::Arc<N> {
	type Output = O;

	fn eval(&'i self, input: I) -> Self::Output {
		(*self.as_ref()).eval(input)
	}
}
impl<'i, 's: 'i, I: 'i, O: 'i, N: Node<'i, I, Output = O>> Node<'i, I> for &'i N {
	type Output = O;

	fn eval(&'i self, input: I) -> Self::Output {
		(**self).eval(input)
	}
}
#[cfg(feature = "alloc")]
impl<'i, I: 'i, O: 'i, N: Node<'i, I, Output = O>> Node<'i, I> for Box<N> {
	type Output = O;

	fn eval(&'i self, input: I) -> Self::Output {
		(**self).eval(input)
	}
}

/*
// Specifically implement for trait objects because they would otherwise evaluated as unsized objetcs
impl<'i, I: 'i, O: 'i> Node<'i, I> for &'i dyn Node<'i, I, Output = O> {
	type Output = O;

	fn eval(&'i self, input: I) -> Self::Output {
		(**self).eval(input)
	}
}*/

pub use crate::application_io::{ExtractImageFrame, SurfaceFrame, SurfaceId};
#[cfg(feature = "wasm")]
pub use application_io::{wasm_application_io, wasm_application_io::WasmEditorApi as EditorApi};
