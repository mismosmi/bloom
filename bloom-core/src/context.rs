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
/// let app: Element<HtmlNode, ()> = Provider::new(Theme("dark")).children(vec![rsx!(<Themed />)]);
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     app,
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>dark</div>".to_string()));
/// ```
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
