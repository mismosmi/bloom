use std::sync::Arc;

use crate::DomRef;

#[derive(Debug, PartialEq, Clone)]
pub struct HtmlComment {
    pub(crate) text: String,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl HtmlComment {
    pub fn new() -> HtmlCommentBuilder<()> {
        HtmlCommentBuilder {
            text: (),
            dom_ref: None,
        }
    }

    pub fn text(&self) -> &String {
        &self.text
    }

    pub fn dom_ref(&self) -> &Option<Arc<DomRef>> {
        &self.dom_ref
    }
}

pub struct HtmlCommentBuilder<T> {
    pub(crate) text: T,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl<T> HtmlCommentBuilder<T> {
    pub fn dom_ref(mut self, dom_ref: Arc<DomRef>) -> Self {
        self.dom_ref = Some(dom_ref);
        self
    }
}

impl HtmlCommentBuilder<()> {
    pub fn text<T>(self, text: T) -> HtmlCommentBuilder<String>
    where
        T: Into<String>,
    {
        HtmlCommentBuilder {
            text: text.into(),
            dom_ref: self.dom_ref,
        }
    }
}

impl HtmlCommentBuilder<String> {
    pub fn build(self) -> HtmlComment {
        HtmlComment {
            text: self.text,
            dom_ref: self.dom_ref,
        }
    }
}
