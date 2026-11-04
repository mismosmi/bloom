use std::{collections::HashMap, fmt::Debug, sync::Arc};

use bloom_core::Element;
use crossterm::event::Event;

use crate::NodeRef;

/// Event handlers are closures that take a crossterm [Event] as an argument.
pub type EventHandler = Box<dyn Fn(Event) + Send + Sync + 'static>;

/// The Node-type to use bloom in the terminal.
/// A TuiNode is either an element (`<box>`, `<column>`, `<text>`, ...) or a text-node.
///
/// TuiNodes will be mainly constructed using bloom-rsx:
/// ```
/// use bloom_core::Element;
/// use bloom_rsx::rsx;
/// use bloom_tui::{prelude::*, TuiNode};
///
/// let element: Element<TuiNode, ()> = rsx!(
///     <column border="rounded" padding=1>
///         <text bold color="green">"Hello, terminal!"</text>
///     </column>
/// );
/// ```
#[derive(Debug, PartialEq, Clone)]
pub enum TuiNode {
    Element(Arc<TuiElement>),
    Text(String),
}

impl TuiNode {
    pub fn element(tag_name: &'static str) -> TuiElementBuilder {
        TuiElement::builder(tag_name)
    }

    pub fn text(text: String) -> Self {
        Self::Text(text)
    }

    pub fn as_element(&self) -> Option<&TuiElement> {
        match self {
            Self::Element(element) => Some(element),
            Self::Text(_) => None,
        }
    }

    pub fn children<E>(self, children: Vec<Element<TuiNode, E>>) -> Element<TuiNode, E> {
        Element::Node(self, children)
    }
}

impl From<String> for TuiNode {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<TuiElement> for TuiNode {
    fn from(element: TuiElement) -> Self {
        Self::Element(Arc::new(element))
    }
}

impl<E> From<TuiNode> for Element<TuiNode, E> {
    fn from(value: TuiNode) -> Self {
        Element::Node(value, Vec::new())
    }
}

impl<E> From<TuiElement> for Element<TuiNode, E> {
    fn from(element: TuiElement) -> Self {
        Element::Node(TuiNode::Element(Arc::new(element)), Vec::new())
    }
}

/// Represents a terminal element such as `<box>`, `<column>` or `<text>`.
/// Attributes are stored as strings, the same way HTML attributes are,
/// and are interpreted by the renderer (see the crate documentation for the supported ones).
pub struct TuiElement {
    pub(crate) tag_name: &'static str,
    pub(crate) attributes: HashMap<String, String>,
    pub(crate) callbacks: HashMap<String, EventHandler>,
    pub(crate) node_ref: Option<Arc<NodeRef>>,
}

impl Debug for TuiElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuiElement")
            .field("tag_name", &self.tag_name)
            .field("attributes", &self.attributes)
            .field("callbacks", &self.callbacks.keys().collect::<Vec<_>>())
            .field("node_ref", &self.node_ref)
            .finish()
    }
}

impl PartialEq for TuiElement {
    fn eq(&self, other: &Self) -> bool {
        self.tag_name == other.tag_name
            && self.attributes == other.attributes
            && self.callbacks.is_empty()
            && other.callbacks.is_empty()
            && self.node_ref == other.node_ref
    }
}

impl TuiElement {
    /// Build a new TuiElement
    /// ```
    /// use bloom_tui::TuiElement;
    ///
    /// let column = TuiElement::builder("column").attr("gap", 1).build();
    /// assert_eq!(column.tag_name(), "column");
    /// assert_eq!(column.attributes().get("gap").map(String::as_str), Some("1"));
    /// ```
    pub fn builder(tag_name: &'static str) -> TuiElementBuilder {
        TuiElementBuilder {
            element: TuiElement {
                tag_name,
                attributes: HashMap::new(),
                callbacks: HashMap::new(),
                node_ref: None,
            },
        }
    }

    pub fn tag_name(&self) -> &str {
        self.tag_name
    }

    pub fn attributes(&self) -> &HashMap<String, String> {
        &self.attributes
    }

    pub fn callbacks(&self) -> &HashMap<String, EventHandler> {
        &self.callbacks
    }

    pub fn node_ref(&self) -> &Option<Arc<NodeRef>> {
        &self.node_ref
    }

    pub fn children<E>(self, children: Vec<Element<TuiNode, E>>) -> Element<TuiNode, E> {
        Element::Node(TuiNode::Element(Arc::new(self)), children)
    }
}

pub struct TuiElementBuilder {
    element: TuiElement,
}

impl TuiElementBuilder {
    /// Set one attribute of the element.
    /// Any value that implements `ToString` is accepted, so `rsx!(<box padding=1 bold />)` works.
    pub fn attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: ToString,
    {
        self.element
            .attributes
            .insert(key.into(), value.to_string());
        self
    }

    /// Set an event handler, `on_key` in rsx becomes `.on("key", handler)`.
    pub fn on<K, C>(mut self, key: K, handler: C) -> Self
    where
        K: Into<String>,
        C: Fn(Event) + Send + Sync + 'static,
    {
        self.element.callbacks.insert(key.into(), Box::new(handler));
        self
    }

    /// Get a reference to the element's position on screen,
    /// `ref={my_ref}` in rsx becomes `.dom_ref(my_ref)`.
    pub fn dom_ref(mut self, node_ref: Arc<NodeRef>) -> Self {
        self.element.node_ref = Some(node_ref);
        self
    }

    pub fn build(self) -> TuiElement {
        self.element
    }
}

/// The `tag`-function rsx uses to build elements.
pub fn tag(tag_name: &'static str) -> TuiElementBuilder {
    TuiElement::builder(tag_name)
}
