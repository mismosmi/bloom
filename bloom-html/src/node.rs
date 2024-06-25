use std::collections::HashMap;

use bloom_core::Element;
use derive_builder::Builder;

#[derive(Builder)]
#[builder(pattern = "immutable")]
pub struct HtmlElement {
    pub(crate) tag_name: String,
    #[builder(default)]
    pub(crate) attributes: HashMap<String, String>,
}

impl HtmlElement {
    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    pub fn attributes(&self) -> &HashMap<String, String> {
        &self.attributes
    }
}

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
}
