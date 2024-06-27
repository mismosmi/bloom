use crate::{node::HtmlElementBuilder, HtmlNode};

pub fn div() -> HtmlElementBuilder {
    HtmlNode::element("div".to_string())
}

pub fn span() -> HtmlElementBuilder {
    HtmlNode::element("span".to_string())
}

pub fn button() -> HtmlElementBuilder {
    HtmlNode::element("button".to_string())
}
