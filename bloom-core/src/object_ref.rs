use std::sync::Arc;

use async_context::with_async_context_mut;

use crate::hook::Hook;

/// use_ref can be used to obtain a persistent reference to an object.
/// The object returned from ref is guaranteed to be the same object
/// on every subsequent call to a component's render-function.
/// The most common use case is with HtmlNodes:
/// ```
/// # use std::sync::Arc;
/// # use async_trait::async_trait;
/// # use bloom_core::{Component, Element};
/// # use bloom_html::{prelude::*, HtmlNode};
/// use bloom_core::{use_effect, use_ref};
/// use bloom_html::DomRef;
/// use bloom_rsx::rsx;
///
/// # #[derive(PartialEq)]
/// # struct MyComponent;
/// # #[async_trait]
/// # impl Component for MyComponent {
/// #     type Node = HtmlNode;
/// #     type Error = ();
/// #     async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, ()>, ()> {
/// let my_div = use_ref::<DomRef>();
///
/// // This can be used in an effect to manipulate the DOM directly:
/// use_effect(my_div.clone(), |my_div| {
///     if let Some(node) = my_div.get() {
///         node.set_text_content(Some("Hello, world!"));
///     }
/// });
///
/// Ok(rsx!(
///     <div ref={my_div} />
/// ))
/// #     }
/// # }
/// ```
/// where my_div will contain a reference to the div element after it has been rendered.
pub fn use_ref<T>() -> Arc<T>
where
    T: Default + Send + Sync + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Arc::new(T::default()));
            hook.ref_index += 1;
            object_ref
                .clone()
                .downcast()
                .expect("Hook Invariant Violation: Failed to cast ref")
        } else {
            Arc::new(T::default())
        }
    })
}

pub fn use_ref_with_default<T, D>(default: D) -> Arc<T>
where
    T: Send + Sync + 'static,
    D: FnOnce() -> T,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Arc::new(default()));
            hook.ref_index += 1;
            object_ref
                .clone()
                .downcast()
                .expect("Hook Invariant Violation: Failed to cast ref")
        } else {
            Arc::new(default())
        }
    })
}
