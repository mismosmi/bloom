mod component;
mod context;
mod effect;
mod element;
mod hook;
mod object_ref;
mod render_loop;
mod render_queue;
mod render_stream;
mod result;
mod state;
mod suspense;

/// Turns a function into a component.
/// The function arguments become the props of the component and the
/// generated builder accepts anything that converts into the prop type.
/// ```
/// use bloom_core::{component, Element};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[component]
/// pub fn Greeting(name: String, times: usize) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<div>{name.repeat(*times)}</div>))
/// }
///
/// #[component]
/// fn Labelled<T: ToString + PartialEq + Send + Sync + 'static>(
///     label: T,
/// ) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<span>{label.to_string()}</span>))
/// }
///
/// #[component]
/// fn App() -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(
///         <Greeting times=2usize name="Hi" />
///         <Labelled<i32> label=42 />
///     ))
/// }
///
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     App::new().build().into(),
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>HiHi</div><span>42</span>".to_string()));
/// ```
///
/// Leaving out a prop is a compile-time error:
/// ```compile_fail
/// # use bloom_core::{component, Element};
/// # use bloom_html::{prelude::*, HtmlNode};
/// # use bloom_rsx::rsx;
/// #[component]
/// fn Greeting(name: String) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<div>{name.clone()}</div>))
/// }
///
/// let greeting = Greeting::new().build();
/// ```
pub use bloom_macro::component;
pub use component::Component;
pub use context::{_get_context, use_context, Provider};
pub use effect::use_effect;
pub use element::Element;
pub use object_ref::{use_ref, use_ref_with_default};
pub use render_loop::{render_loop, ObjectModel};
pub use render_stream::{render_stream, NodeStream};
pub use result::Result;
pub use state::use_state;

#[doc(hidden)]
pub mod __private {
    pub use async_trait::async_trait;
}
