use futures::Future;

use graphene_core::Node;

use std::cell::UnsafeCell;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use xxhash_rust::xxh3::Xxh3;

/// Caches the output of a given Node and acts as a proxy
#[derive(Default)]
pub struct CacheNode<T, CachedNode> {
	cache: UnsafeCell<Option<T>>,
	node: CachedNode,
}
impl<'i, 'o: 'i, T: 'i + Clone + 'o, CachedNode: 'i> Node<'i, ()> for CacheNode<T, CachedNode>
where
	CachedNode: for<'any_input> Node<'any_input, ()>,
	for<'a> <CachedNode as Node<'a, ()>>::Output: core::future::Future<Output = T> + 'a,
{
	// TODO: This should return a reference to the cached cached_value
	// but that requires a lot of lifetime magic <- This was suggested by copilot but is pretty acurate xD
	type Output = Pin<Box<dyn Future<Output = &'i T> + 'i>>;
	fn eval(&'i self, input: ()) -> Self::Output {
		Box::pin(async move {
			if let Some(ref cached_value) = unsafe { &*self.cache.get() } {
				cached_value
			} else {
				let value = self.node.eval(input).await;
				unsafe {
					*self.cache.get() = Some(value);
					(&*self.cache.get()).as_ref().unwrap()
				}
			}
		})
	}

	unsafe fn reset(&self) {
		*self.cache.get() = None;
	}
}

impl<T, CachedNode> std::marker::Unpin for CacheNode<T, CachedNode> {}

impl<T, CachedNode> CacheNode<T, CachedNode> {
	pub fn new(node: CachedNode) -> CacheNode<T, CachedNode> {
		CacheNode { cache: Default::default(), node }
	}
}

/// Caches the output of the last graph evaluation for introspection
#[derive(Default)]
pub struct MonitorNode<T> {
	output: Mutex<Option<Arc<T>>>,
}
impl<'i, T: 'static + Clone> Node<'i, T> for MonitorNode<T> {
	type Output = T;
	fn eval(&'i self, input: T) -> Self::Output {
		*self.output.lock().unwrap() = Some(Arc::new(input.clone()));
		input
	}

	fn serialize(&self) -> Option<Arc<dyn core::any::Any>> {
		let output = self.output.lock().unwrap();
		(*output).as_ref().map(|output| output.clone() as Arc<dyn core::any::Any>)
	}
}

impl<T> MonitorNode<T> {
	pub const fn new() -> MonitorNode<T> {
		MonitorNode { output: Mutex::new(None) }
	}
}

/// Caches the output of a given Node and acts as a proxy
/// It provides two modes of operation, it can either be set
/// when calling the node with a `Some<T>` variant or the last
/// value that was added is returned when calling it with `None`
#[derive(Debug, Default)]
pub struct LetNode<T> {
	// We have to use an append only data structure to make sure the references
	// to the cache entries are always valid
	// TODO: We only ever access the last value so there is not really a reason for us
	// to store the previous entries. This should be reworked in the future
	cache: UnsafeCell<Option<T>>,
}
impl<'i, T: 'i> Node<'i, Option<T>> for LetNode<T> {
	type Output = &'i T;
	fn eval(&'i self, input: Option<T>) -> Self::Output {
		unsafe {
			if let Some(input) = input {
				*self.cache.get() = Some(input);
			}
			(*self.cache.get()).as_ref().expect("LetNode was not initialized")
		}
	}
	unsafe fn reset(&self) {
		*self.cache.get() = None;
	}
}

impl<T> std::marker::Unpin for LetNode<T> {}

impl<T> LetNode<T> {
	pub fn new() -> LetNode<T> {
		LetNode { cache: Default::default() }
	}
}

/// Caches the output of a given Node and acts as a proxy
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndLetNode<Input> {
	input: Input,
}
impl<'i, T: 'i, Input> Node<'i, &'i T> for EndLetNode<Input>
where
	Input: Node<'i, ()>,
{
	type Output = <Input>::Output;
	fn eval(&'i self, _: &'i T) -> Self::Output {
		self.input.eval(())
	}
}

impl<Input> EndLetNode<Input> {
	pub const fn new(input: Input) -> EndLetNode<Input> {
		EndLetNode { input }
	}
}

pub use graphene_core::ops::SomeNode as InitNode;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RefNode<T, Let> {
	let_node: Let,
	_t: PhantomData<T>,
}

impl<'i, T: 'i, Let> Node<'i, ()> for RefNode<T, Let>
where
	Let: for<'a> Node<'a, Option<T>>,
{
	type Output = <Let as Node<'i, Option<T>>>::Output;
	fn eval(&'i self, _: ()) -> Self::Output {
		self.let_node.eval(None)
	}
}

impl<Let, T> RefNode<T, Let> {
	pub const fn new(let_node: Let) -> RefNode<T, Let> {
		RefNode { let_node, _t: PhantomData }
	}
}
