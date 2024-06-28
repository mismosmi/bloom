use std::{
    any::Any,
    sync::{Arc, RwLock},
};

use async_context::with_async_context_mut;

use crate::hook::Hook;

pub(crate) trait AnyObjectRef {
    fn as_any_ref(&self) -> Arc<RwLock<dyn Any + Send + Sync + 'static>>;
}

pub(crate) fn downcast_object_ref<T>(any_ref: &dyn Any) -> Arc<RwLock<T>>
where
    T: 'static,
{
    let object_ref = any_ref.downcast_ref::<Arc<RwLock<T>>>().unwrap();

    object_ref.clone()
}

pub(crate) fn clone_object_ref(
    object_ref: &Box<dyn AnyObjectRef + Send + Sync + 'static>,
) -> Box<dyn AnyObjectRef + Send + Sync + 'static> {
    Box::new(object_ref.as_any_ref())
}

impl<T> AnyObjectRef for Arc<RwLock<T>>
where
    T: Send + Sync + 'static,
{
    fn as_any_ref(&self) -> Arc<RwLock<dyn Any + Send + Sync + 'static>> {
        self.clone()
    }
}

impl AnyObjectRef for Arc<RwLock<dyn Any + Send + Sync + 'static>> {
    fn as_any_ref(&self) -> Arc<RwLock<dyn Any + Send + Sync + 'static>> {
        self.clone()
    }
}

pub fn use_ref<T>() -> Arc<RwLock<T>>
where
    T: Default + Send + Sync + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Box::new(Arc::new(RwLock::new(T::default()))));
            hook.ref_index += 1;
            downcast_object_ref(object_ref)
        } else {
            Arc::new(RwLock::new(T::default()))
        }
    })
}
