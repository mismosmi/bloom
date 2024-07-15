use std::sync::Arc;

use crate::DomRef;

/// Represents an HTML comment (`<!-- text -->`).
#[derive(Debug, PartialEq, Clone)]
pub struct HtmlComment {
    pub(crate) text: String,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl HtmlComment {
    /// Build a new HTML comment
    /// ```
    /// HtmlComment::new().text("foo").build();
    /// ```
    pub fn new() -> HtmlCommentBuilder<()> {
        HtmlCommentBuilder {
            text: (),
            dom_ref: None,
        }
    }

    /// Get the text-content of the comment
    pub fn text(&self) -> &String {
        &self.text
    }

    /// Get the DOM reference of the comment
    pub fn dom_ref(&self) -> &Option<Arc<DomRef>> {
        &self.dom_ref
    }
}

pub struct HtmlCommentBuilder<T> {
    pub(crate) text: T,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl<T> HtmlCommentBuilder<T> {
    /// Use
    /// ```
    /// let comment = use_ref::<DomRef>();
    /// ```
    /// to obtain the `Arc<DomRef>` to pass to this method.
    pub fn dom_ref(mut self, dom_ref: Arc<DomRef>) -> Self {
        self.dom_ref = Some(dom_ref);
        self
    }
}

impl HtmlCommentBuilder<()> {
    /// set the text-content of the comment
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
    /// build the comment, the use `comment.into()` to convert it to a `bloom_core::Element<HtmlNode, E>`
    pub fn build(self) -> HtmlComment {
        HtmlComment {
            text: self.text,
            dom_ref: self.dom_ref,
        }
    }
}
