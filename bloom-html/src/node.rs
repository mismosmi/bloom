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
use derive_builder::Builder;

use crate::event::{EventHandler, HtmlEvent};

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

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct HtmlElement {
    pub(crate) tag_name: String,
    #[builder(default)]
    pub(crate) attributes: HashMap<String, String>,
    #[builder(default)]
    pub(crate) callbacks: HashMap<String, EventHandler>,
    #[builder(default, setter(into))]
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

#[derive(Debug)]
pub enum HtmlNode {
    Element(HtmlElement),
    Text(String),
}

impl HtmlNode {
    pub fn element(tag_name: String) -> HtmlElementBuilder {
        HtmlElementBuilder::default().tag_name(tag_name)
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

impl<E> From<HtmlElementBuilder> for Element<HtmlNode, E> {
    fn from(builder: HtmlElementBuilder) -> Self {
        Element::Node(
            HtmlNode::Element(builder.build().expect("Missing Tag Name")),
            Vec::new(),
        )
    }
}

impl From<HtmlElementBuilder> for HtmlNode {
    fn from(builder: HtmlElementBuilder) -> Self {
        HtmlNode::Element(builder.build().expect("Missing Tag Name"))
    }
}

impl HtmlElementBuilder {
    pub fn children<E>(self, children: Vec<Element<HtmlNode, E>>) -> Element<HtmlNode, E> {
        Element::Node(
            HtmlNode::Element(self.build().expect("Missing Tag Name")),
            children,
        )
    }

    pub fn attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: ToString,
        V: ToString,
    {
        self.attributes
            .get_or_insert(HashMap::new())
            .insert(key.to_string(), value.to_string());
        self
    }

    pub fn on<K, C>(mut self, key: K, callback: C) -> Self
    where
        K: ToString,
        C: Fn(HtmlEvent) + Send + Sync + 'static,
    {
        self.callbacks
            .get_or_insert(HashMap::new())
            .insert(key.to_string(), Box::new(callback));
        self
    }
}
