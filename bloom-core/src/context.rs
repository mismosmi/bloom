use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

use async_context::with_async_context;

use crate::{hook::Hook, Element};

/// Provides a value to all components below it, which can read it with [use_context].
/// ```
/// use bloom_core::{component, use_context, Element, Provider};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[derive(Default)]
/// struct Theme(&'static str);
///
/// #[component]
/// fn Themed() -> Result<Element<HtmlNode, ()>, ()> {
///     let theme = use_context::<Theme>();
///     Ok(rsx!(<div>{theme.0}</div>))
/// }
///
/// let app: Element<HtmlNode, ()> = rsx!(<Provider value={Theme("dark")}><Themed /></Provider>);
///
/// // Without rsx
/// let app: Element<HtmlNode, ()> = Provider::builder()
///     .value(Theme("dark"))
///     .children(vec![rsx!(<Themed />)])
///     .build()
///     .into();
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     app,
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>dark</div>".to_string()));
/// ```
///
/// A provider needs a value:
/// ```compile_fail
/// use bloom_core::{Element, Provider};
/// use bloom_html::HtmlNode;
///
/// let provider: Element<HtmlNode, ()> = Provider::builder().build().into();
/// ```
pub struct Provider<N, E>
where
    N: From<String>,
{
    value: Arc<dyn Any + Send + Sync>,
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
    pub fn value<T>(self, value: T) -> ProviderBuilder<N, E, Arc<dyn Any + Send + Sync>>
    where
        T: Send + Sync + 'static,
    {
        ProviderBuilder {
            value: Arc::new(value),
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

impl<N, E> ProviderBuilder<N, E, Arc<dyn Any + Send + Sync>>
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

pub(crate) type ContextMap = Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>;

/// Read the value of the closest [Provider] above the component.
/// All consumers share the provided value, it is never cloned.
/// Without a provider, a default value is returned.
pub fn use_context<T>() -> Arc<T>
where
    T: Default + Send + Sync + 'static,
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
