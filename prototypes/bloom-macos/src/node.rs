//! Platform-independent node description, the native analogue of bloom-html.
//! Builder shape mirrors HtmlElementBuilder so `rsx!` works unchanged.
use std::{collections::HashMap, fmt::Debug, sync::Arc};

use bloom_core::Element;

/// What the native side reports to a handler. Deliberately not tied to AppKit
/// so node descriptions stay `Send + Sync` and testable off-macOS.
#[derive(Debug, Clone)]
pub enum NativeEvent {
    Click,
    Change(String),
}

pub type NativeHandler = Box<dyn Fn(NativeEvent) + Send + Sync + 'static>;

pub struct NativeElement {
    pub(crate) tag: &'static str,
    pub(crate) attributes: HashMap<String, String>,
    pub(crate) callbacks: HashMap<String, NativeHandler>,
}

impl NativeElement {
    pub fn tag(&self) -> &'static str {
        self.tag
    }
    pub fn attributes(&self) -> &HashMap<String, String> {
        &self.attributes
    }
    pub fn callbacks(&self) -> &HashMap<String, NativeHandler> {
        &self.callbacks
    }
}

impl Debug for NativeElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeElement")
            .field("tag", &self.tag)
            .field("attributes", &self.attributes)
            .finish()
    }
}

impl PartialEq for NativeElement {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
            && self.attributes == other.attributes
            && self.callbacks.is_empty()
            && other.callbacks.is_empty()
    }
}

#[derive(Debug, PartialEq)]
pub enum NativeNode {
    Element(Arc<NativeElement>),
    /// Bare strings in rsx become labels.
    Text(String),
}

impl From<String> for NativeNode {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

pub struct NativeElementBuilder {
    tag: &'static str,
    attributes: HashMap<String, String>,
    callbacks: HashMap<String, NativeHandler>,
}

pub fn tag(tag: &'static str) -> NativeElementBuilder {
    NativeElementBuilder {
        tag,
        attributes: HashMap::new(),
        callbacks: HashMap::new(),
    }
}

impl NativeElementBuilder {
    pub fn attr(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.attributes.insert(key.into(), value.to_string());
        self
    }

    pub fn on<C>(mut self, event: impl Into<String>, handler: C) -> Self
    where
        C: Fn(NativeEvent) + Send + Sync + 'static,
    {
        self.callbacks.insert(event.into(), Box::new(handler));
        self
    }

    pub fn build(self) -> NativeElement {
        NativeElement {
            tag: self.tag,
            attributes: self.attributes,
            callbacks: self.callbacks,
        }
    }
}

impl NativeElement {
    pub fn children<E>(self, children: Vec<Element<NativeNode, E>>) -> Element<NativeNode, E> {
        Element::Node(NativeNode::Element(Arc::new(self)), children)
    }
}

impl<E> From<NativeElement> for Element<NativeNode, E> {
    fn from(element: NativeElement) -> Self {
        Element::Node(NativeNode::Element(Arc::new(element)), Vec::new())
    }
}

pub mod prelude {
    pub use super::tag;
}
