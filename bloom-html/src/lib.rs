//! HTML nodes and examples for Bloom.
//!
//! ## Fragments
//!
//! Groups elements without adding a node.
//! In rsx it is used like a component, and like any other element it can be keyed,
//! e.g. to key several nodes at once:
//! ```
//! use bloom_core::{Element, Fragment};
//! use bloom_html::{prelude::*, HtmlNode};
//! use bloom_rsx::rsx;
//!
//! let items = vec![(1, "one"), (2, "two")];
//! let list: Element<HtmlNode, ()> = rsx!(
//!     <dl>{items.iter().map(|(id, label)| rsx!(
//!         <Fragment key={id}><dt>{id.to_string()}</dt><dd>{*label}</dd></Fragment>
//!     )).collect::<Vec<_>>()}</dl>
//! );
//!
//! // Without rsx
//! let group: Element<HtmlNode, ()> = Fragment::builder().children(vec!["one".into()]).build().into();
//! ```
//!
//! ## DOM references
//!
//! The most common use case is with HtmlNodes:
//! ```
//! # use std::sync::Arc;
//! # use async_trait::async_trait;
//! # use bloom_core::{Component, Element};
//! # use bloom_html::{prelude::*, HtmlNode};
//! use bloom_core::{use_effect, use_ref};
//! use bloom_html::DomRef;
//! use bloom_rsx::rsx;
//!
//! # #[derive(PartialEq)]
//! # struct MyComponent;
//! # #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), async_trait(?Send))]
//! # #[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), async_trait)]
//! # impl Component for MyComponent {
//! #     type Node = HtmlNode;
//! #     type Error = ();
//! #     async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, ()>, ()> {
//! let my_div = use_ref::<DomRef>();
//!
//! // This can be used in an effect to manipulate the DOM directly:
//! use_effect(my_div.clone(), |my_div| {
//!     if let Some(node) = my_div.get() {
//!         node.set_text_content(Some("Hello, world!"));
//!     }
//! });
//!
//! Ok(rsx!(
//!     <div ref={my_div} />
//! ))
//! #     }
//! # }
//! ```
//! where my_div will contain a reference to the div element after it has been rendered.
//!
//! ## State updates
//!
//! Its update-method can be used to change the state.
//! This will trigger a re-render of the component.
//! ```
//! # use std::sync::Arc;
//! # use async_trait::async_trait;
//! # use bloom_core::{use_state, Component, Element};
//! # use bloom_html::{prelude::*, HtmlNode};
//! # use bloom_rsx::rsx;
//! # #[derive(PartialEq)]
//! # struct Counter;
//! # #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), async_trait(?Send))]
//! # #[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), async_trait)]
//! # impl Component for Counter {
//! #     type Node = HtmlNode;
//! #     type Error = ();
//! #     async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, ()>, ()> {
//! let my_state = use_state(|| 0.into());
//!
//! Ok(rsx!(
//!     <button on_click={move |_| my_state.update(|value| (*value + 1).into())}>"Increment"</button>
//! ))
//! #     }
//! # }
//! ```

mod comment;
mod dom_ref;
mod element;
mod event;
mod node;
pub mod tag;

use bloom_core::Element;
pub use comment::HtmlComment;
pub use dom_ref::DomRef;
pub use element::HtmlElement;
pub use event::EventHandler;
pub use node::{HtmlNode, tag};

/// shortcut for generating text-nodes
pub fn text<E, T>(text: T) -> Element<HtmlNode, E>
where
    T: ToString,
{
    Element::Node(HtmlNode::text(text.to_string()), Vec::new())
}

/// Make sure to import `bloom_html::prelude::*` wherever you want to use (https://crates.io/crates/bloom-rsx)[bloom-rsx]
/// to render HtmlNodes
pub mod prelude {
    /// The `tag`-function rsx will use to generate HtmlElements
    pub use super::tag;
    /// The `keyed`-function rsx will use for elements with a `key`
    pub use bloom_core::keyed;
}
