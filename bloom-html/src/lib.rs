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

pub fn text<E, T>(text: T) -> Element<HtmlNode, E>
where
    T: ToString,
{
    Element::Node(HtmlNode::text(text.to_string()), Vec::new())
}

pub mod prelude {
    pub use super::tag;
}
