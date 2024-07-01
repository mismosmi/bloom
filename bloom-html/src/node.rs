use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::Debug,
    hash::Hash,
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
};

use bloom_core::Element;

use crate::event::EventHandler;

#[derive(Debug, Default)]
pub struct DomRef(AtomicU16);

thread_local! {
    static HTML_ELEMENT_MAP: RefCell<HashMap<u16, web_sys::Element>> = RefCell::new(HashMap::new());
}

impl DomRef {
    pub fn set(&self, element: web_sys::Element) {
        HTML_ELEMENT_MAP.with(|map| {
            let mut map = map.borrow_mut();
            let current_key = self.0.load(Ordering::Relaxed);
            let key = if current_key != 0 {
                current_key
            } else {
                let mut key = 1;
                while map.contains_key(&key) {
                    key += 1;
                    if key == u16::MAX {
                        panic!("Element Map Overflow");
                    }
                }
                self.0.store(key, Ordering::Relaxed);
                key
            };
            map.insert(key, element);
        });
    }

    pub fn get(&self) -> Option<web_sys::Element> {
        HTML_ELEMENT_MAP.with(|map| {
            map.borrow()
                .get(&self.0.load(Ordering::Relaxed))
                .map(|element| element.clone())
        })
    }
}

impl Drop for DomRef {
    fn drop(&mut self) {
        HTML_ELEMENT_MAP.with(|map| {
            map.borrow_mut().remove(&self.0.load(Ordering::Relaxed));
        });
    }
}

impl Hash for DomRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.load(Ordering::Relaxed).hash(state);
    }
}

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

#[derive(Debug)]
pub enum HtmlNode {
    Element(HtmlElement),
    Text(String),
}

impl HtmlNode {
    pub fn element(tag_name: &'static str) -> HtmlElementBuilder<&'static str> {
        HtmlElement::new().tag_name(tag_name)
    }

    pub fn text(text: String) -> Self {
        Self::Text(text)
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
        Element::Node(HtmlNode::Element(element), Vec::new())
    }
}

impl From<HtmlElement> for HtmlNode {
    fn from(element: HtmlElement) -> Self {
        HtmlNode::Element(element)
    }
}

impl HtmlElement {
    pub fn children<E>(self, children: Vec<Element<HtmlNode, E>>) -> Element<HtmlNode, E> {
        Element::Node(HtmlNode::Element(self), children)
    }
}

impl HtmlNode {
    pub fn children<E>(self, children: Vec<Element<HtmlNode, E>>) -> Element<HtmlNode, E> {
        Element::Node(self, children)
    }
}
