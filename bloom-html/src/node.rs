use std::{fmt::Debug, sync::Arc};

use bloom_core::Element;

use crate::{
    comment::{HtmlComment, HtmlCommentBuilder},
    element::HtmlElementBuilder,
    HtmlElement,
};

/// The Node-type to use bloom in Browser-Environments.
/// A HtmlNode is equivalent to a browser DOM-node.
/// It can represent an HTML Element (<div>, <span>, etc.),
/// a text-node or a comment.
///
/// HtmlNodes will be mainly constructed using bloom-rsx:
/// ```
/// rsx!(<div id="123" on_click=|_| { alert!("clicked")} />)
/// ```
#[derive(Debug, PartialEq, Clone)]
pub enum HtmlNode {
    Element(Arc<HtmlElement>),
    Text(String),
    Comment(HtmlComment),
}

impl HtmlNode {
    pub fn element(tag_name: &'static str) -> HtmlElementBuilder<&'static str> {
        HtmlElement::new().tag_name(tag_name)
    }

    pub fn text(text: String) -> Self {
        Self::Text(text)
    }

    pub fn comment(text: String) -> HtmlCommentBuilder<String> {
        HtmlComment::new().text(text)
    }

    pub fn as_element(&self) -> Option<&HtmlElement> {
        match self {
            Self::Element(element) => Some(element),
            _ => None,
        }
    }
}

impl<E> From<HtmlElement> for Element<HtmlNode, E> {
    fn from(element: HtmlElement) -> Self {
        Element::Node(HtmlNode::Element(Arc::new(element)), Vec::new())
    }
}

impl<E> From<HtmlComment> for Element<HtmlNode, E> {
    fn from(comment: HtmlComment) -> Self {
        Element::Node(HtmlNode::Comment(comment), Vec::new())
    }
}

impl From<HtmlElement> for HtmlNode {
    fn from(element: HtmlElement) -> Self {
        HtmlNode::Element(Arc::new(element))
    }
}

impl From<HtmlComment> for HtmlNode {
    fn from(value: HtmlComment) -> Self {
        HtmlNode::Comment(value)
    }
}

impl HtmlElement {
    pub fn children<E>(self, children: Vec<Element<HtmlNode, E>>) -> Element<HtmlNode, E> {
        Element::Node(HtmlNode::Element(Arc::new(self)), children)
    }
}

impl HtmlNode {
    pub fn children<E>(self, children: Vec<Element<HtmlNode, E>>) -> Element<HtmlNode, E> {
        Element::Node(self, children)
    }
}

impl From<String> for HtmlNode {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl<E> From<HtmlNode> for Element<HtmlNode, E> {
    fn from(value: HtmlNode) -> Self {
        Element::Node(value, Vec::new())
    }
}

pub fn tag(tag_name: &'static str) -> HtmlElementBuilder<&'static str> {
    HtmlElement::new().tag_name(tag_name)
}
