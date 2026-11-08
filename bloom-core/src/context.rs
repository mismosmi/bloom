use crate::{MaybeSend, MaybeSync};
use std::{any::TypeId, collections::HashMap, sync::Arc};

use async_context::with_async_context;

use crate::{Element, hook::Hook};

/// Provides a value to all components below it, which can read it with [use_context].
/// ```
/// use bloom_core::{component, use_context, Element, Provider};
/// #[derive(Default)]
/// struct Theme(&'static str);
/// #[component]
/// fn Themed() -> Result<Element<String, ()>, ()> {
///     Ok(use_context::<Theme>().0.into())
/// }
/// let app: Element<String, ()> = Provider::builder()
///     .value(Theme("dark"))
///     .children(vec![Themed::builder().build().into()])
///     .build()
///     .into();
/// ```
///
/// A provider needs a value:
/// ```compile_fail,E0599
/// use bloom_core::{Element, Provider};
/// let provider: Element<String, ()> = Provider::builder().build().into();
/// ```
pub struct Provider<N, E>
where
    N: From<String>,
{
    value: crate::platform::SharedValue,
    children: Vec<Element<N, E>>,
}

impl<N, E> Provider<N, E>
where
    N: From<String>,
{
    pub fn builder() -> ProviderBuilder<N, E, ()> {
        ProviderBuilder {
            value: (),
            children: Vec::new(),
        }
    }
}

pub struct ProviderBuilder<N, E, V>
where
    N: From<String>,
{
    value: V,
    children: Vec<Element<N, E>>,
}

impl<N, E> ProviderBuilder<N, E, ()>
where
    N: From<String>,
{
    pub fn value<T>(self, value: T) -> ProviderBuilder<N, E, crate::platform::SharedValue>
    where
        T: MaybeSend + MaybeSync + 'static,
    {
        ProviderBuilder {
            value: Arc::new(value).into(),
            children: self.children,
        }
    }
}

impl<N, E, V> ProviderBuilder<N, E, V>
where
    N: From<String>,
{
    pub fn children(mut self, children: Vec<Element<N, E>>) -> Self {
        self.children = children;
        self
    }
}

impl<N, E> ProviderBuilder<N, E, crate::platform::SharedValue>
where
    N: From<String>,
{
    pub fn build(self) -> Provider<N, E> {
        Provider {
            value: self.value,
            children: self.children,
        }
    }
}

impl<N, E> From<Provider<N, E>> for Element<N, E>
where
    N: From<String>,
{
    fn from(provider: Provider<N, E>) -> Self {
        Element::Provider(provider.value, provider.children)
    }
}

pub(crate) type ContextMap = Arc<HashMap<TypeId, crate::platform::SharedValue>>;

/// Read the value of the closest [Provider] above the component.
/// All consumers share the provided value, it is never cloned.
/// Without a provider, a default value is returned.
pub fn use_context<T>() -> Arc<T>
where
    T: Default + MaybeSend + MaybeSync + 'static,
{
    with_async_context(|hook: Option<&Hook>| {
        if let Some(hook) = hook {
            hook.context
                .get(&TypeId::of::<T>())
                .and_then(|value| value.clone().downcast::<T>().ok())
                .unwrap_or_default()
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
