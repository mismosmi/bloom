//! Thread-safety requirements that are relaxed for browser WebAssembly.
use std::{
    any::{Any, TypeId},
    sync::Arc,
};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
/// Requires `Send` except on browser WebAssembly targets.
pub trait MaybeSend: Send {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Send + ?Sized> MaybeSend for T {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
/// Requires `Send` except on browser WebAssembly targets.
pub trait MaybeSend {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T: ?Sized> MaybeSend for T {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
/// Requires `Sync` except on browser WebAssembly targets.
pub trait MaybeSync: Sync {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Sync + ?Sized> MaybeSync for T {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
/// Requires `Sync` except on browser WebAssembly targets.
pub trait MaybeSync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T: ?Sized> MaybeSync for T {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use futures_util::{
    future::BoxFuture,
    stream::BoxStream,
    task::{FutureObj as TaskFuture, Spawn as MaybeSpawn},
};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use futures_util::{
    future::LocalBoxFuture as BoxFuture,
    stream::LocalBoxStream as BoxStream,
    task::{LocalFutureObj as TaskFuture, LocalSpawn as MaybeSpawn},
};

trait ErasedValue: MaybeSend + MaybeSync {
    fn as_any(&self) -> &dyn Any;
    fn value_type_id(&self) -> TypeId;
}
impl<T: MaybeSend + MaybeSync + 'static> ErasedValue for Arc<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn value_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

/// A shared, type-erased hook or context value.
/// Stores the typed `Arc` so downcasting stays safe on targets where values need not be `Send`.
#[derive(Clone)]
pub struct SharedValue(Arc<dyn ErasedValue>);
impl<T: MaybeSend + MaybeSync + 'static> From<Arc<T>> for SharedValue {
    fn from(value: Arc<T>) -> Self {
        Self(Arc::new(value))
    }
}
impl SharedValue {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn type_id(&self) -> TypeId {
        self.0.value_type_id()
    }
    pub fn downcast<T: 'static>(self) -> Result<Arc<T>, Self> {
        match self.0.as_any().downcast_ref::<Arc<T>>() {
            Some(value) => Ok(value.clone()),
            None => Err(self),
        }
    }
}
impl std::fmt::Debug for SharedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedValue")
            .field("type_id", &self.type_id())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erased_values_preserve_type_and_shared_identity() {
        let original = Arc::new(String::from("value"));
        let erased = SharedValue::from(original.clone());
        assert_eq!(erased.type_id(), TypeId::of::<String>());
        assert!(erased.ptr_eq(&erased.clone()));
        let erased = erased.downcast::<usize>().unwrap_err();
        assert!(Arc::ptr_eq(
            &original,
            &erased.downcast::<String>().unwrap()
        ));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn native_erased_types_keep_thread_safety() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedValue>();
        assert_send_sync::<crate::Spawner>();
        assert_send_sync::<crate::Element<String, ()>>();
    }
}
