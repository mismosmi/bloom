use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

use async_context::{with_async_context, with_async_context_mut};

use crate::{hook::Hook, Element};

pub struct Provider {
    value: Arc<dyn Any + Send + Sync>,
}

impl Provider {
    pub fn new<T>(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            value: Arc::new(value),
        }
    }

    pub fn children<N, E>(self, children: Vec<Element<N, E>>) -> Element<N, E>
    where
        N: From<String>,
    {
        Element::Provider(self.value, children)
    }
}

pub(crate) type ContextMap = Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>;

pub fn use_context<T>() -> Arc<T>
where
    T: Clone + Default + 'static,
{
    with_async_context(|hook: Option<&Hook>| {
        if let Some(hook) = hook {
            hook.context
                .get(&TypeId::of::<T>())
                .and_then(|value| value.downcast_ref::<Arc<T>>())
                .cloned()
                .unwrap_or(Arc::new(T::default()))
        } else {
            Arc::new(T::default())
        }
    })
}

pub fn _get_context() -> ContextMap {
    with_async_context(|hook: Option<&Hook>| {
        if let Some(hook) = hook {
            hook.context.clone()
        } else {
            Arc::default()
        }
    })
}
