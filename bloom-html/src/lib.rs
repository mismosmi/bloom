mod event;
mod node;
pub mod tag;

use bloom_core::Element;
pub use event::{EventHandler, HtmlEvent};
pub use node::{DomRef, HtmlElement, HtmlNode};

pub fn text<E, T>(text: T) -> Element<HtmlNode, E>
where
    T: ToString,
{
    Element::Node(HtmlNode::text(text.to_string()), Vec::new())
}
