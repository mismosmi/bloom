//! Some basic html tags
use crate::{HtmlNode, element::HtmlElementBuilder};

pub fn div() -> HtmlElementBuilder<&'static str> {
    HtmlNode::element("div")
}

pub fn span() -> HtmlElementBuilder<&'static str> {
    HtmlNode::element("span")
}

pub fn button() -> HtmlElementBuilder<&'static str> {
    HtmlNode::element("button")
}

pub fn script() -> HtmlElementBuilder<&'static str> {
    HtmlNode::element("script")
}
