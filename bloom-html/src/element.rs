use std::{collections::HashMap, fmt::Debug, sync::Arc};

use crate::{DomRef, EventHandler};

pub struct HtmlElement {
    pub(crate) tag_name: &'static str,
    pub(crate) attributes: HashMap<String, String>,
    pub(crate) callbacks: HashMap<String, EventHandler>,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl Debug for HtmlElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HtmlElement")
            .field("tag_name", &self.tag_name)
            .field("attributes", &self.attributes)
            .field("callbacks", &"Callbacks")
            .field("dom_ref", &self.dom_ref)
            .finish()
    }
}

impl HtmlElement {
    pub fn new() -> HtmlElementBuilder<()> {
        HtmlElementBuilder {
            tag_name: (),
            attributes: HashMap::new(),
            callbacks: HashMap::new(),
            dom_ref: None,
        }
    }

    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    pub fn attributes(&self) -> &HashMap<String, String> {
        &self.attributes
    }

    pub fn callbacks(&self) -> &HashMap<String, EventHandler> {
        &self.callbacks
    }

    pub fn dom_ref(&self) -> &Option<Arc<DomRef>> {
        &self.dom_ref
    }
}

pub struct HtmlElementBuilder<T> {
    pub(crate) tag_name: T,
    pub(crate) attributes: HashMap<String, String>,
    pub(crate) callbacks: HashMap<String, EventHandler>,
    pub(crate) dom_ref: Option<Arc<DomRef>>,
}

impl HtmlElementBuilder<()> {
    pub fn tag_name(self, tag_name: &'static str) -> HtmlElementBuilder<&'static str> {
        HtmlElementBuilder {
            tag_name,
            attributes: self.attributes,
            callbacks: self.callbacks,
            dom_ref: self.dom_ref,
        }
    }
}

impl<T> HtmlElementBuilder<T> {
    pub fn attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        V: Into<String>,
    {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub fn on<K, C>(mut self, key: K, handler: C) -> Self
    where
        K: Into<String>,
        C: Fn(web_sys::Event) + Send + Sync + 'static,
    {
        self.callbacks.insert(key.into(), Box::new(handler));
        self
    }

    pub fn dom_ref(mut self, dom_ref: Arc<DomRef>) -> Self {
        self.dom_ref = Some(dom_ref);
        self
    }
}

impl HtmlElementBuilder<&'static str> {
    pub fn build(self) -> HtmlElement {
        HtmlElement {
            tag_name: self.tag_name,
            attributes: self.attributes,
            callbacks: self.callbacks,
            dom_ref: self.dom_ref,
        }
    }
}

impl PartialEq for HtmlElement {
    fn eq(&self, other: &Self) -> bool {
        self.tag_name == other.tag_name
            && self.attributes == other.attributes
            && self.callbacks.is_empty()
            && other.callbacks.is_empty()
            && self.dom_ref == other.dom_ref
    }
}
