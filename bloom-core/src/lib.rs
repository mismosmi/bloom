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
/// The function arguments become the props of the component.
/// Props of type `Option<T>` are optional, all other props are required.
/// The generated builder (from [bon](https://docs.rs/bon)) accepts anything that converts into the prop type.
/// ```
/// use bloom_core::{component, Element};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[component]
/// pub fn Greeting(name: String, times: Option<usize>) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<div>{name.repeat(times.unwrap_or(1))}</div>))
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
///         <Greeting name="Hello" />
///         <Labelled<i32> label=42 />
///     ))
/// }
///
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     App::builder().build().into(),
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>HiHi</div><div>Hello</div><span>42</span>".to_string()));
/// ```
///
/// Leaving out a required prop is a compile-time error:
/// ```compile_fail
/// # use bloom_core::{component, Element};
/// # use bloom_html::{prelude::*, HtmlNode};
/// # use bloom_rsx::rsx;
/// #[component]
/// fn Greeting(name: String) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<div>{name.clone()}</div>))
/// }
///
/// let greeting = Greeting::builder().build();
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
pub use suspense::Suspense;

#[doc(hidden)]
pub mod __private {
    pub use async_trait::async_trait;
    pub use bon;
}
