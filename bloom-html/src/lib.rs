mod comment;
mod dom_ref;
mod element;
mod event;
mod node;
pub mod tag;

use bloom_core::Element;
pub use dom_ref::DomRef;
pub use element::HtmlElement;
pub use event::EventHandler;
pub use node::{tag, HtmlNode};

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
}
