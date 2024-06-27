use std::{collections::HashMap, fmt::Debug};

use bloom_core::Element;
use derive_builder::Builder;

use crate::event::{EventHandler, HtmlEvent};

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct HtmlElement {
    pub(crate) tag_name: String,
    #[builder(default)]
    pub(crate) attributes: HashMap<String, String>,
    #[builder(default)]
    pub(crate) callbacks: HashMap<String, EventHandler>,
}

impl Debug for HtmlElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HtmlElement")
            .field("tag_name", &self.tag_name)
            .field("attributes", &self.attributes)
            .field("callbacks", &"Callbacks")
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
