use std::{any::Any, hash::Hash, sync::Arc};

use crate::{
    component::{AnyComponent, ComponentDiff},
    Key,
};

/// The element type is returned from component render functions.
/// It can be constructed from text or a component, or directly with a native node.
/// ```
/// use bloom_core::Element;
/// let text: Element<String, ()> = "Hello".into();
/// let node: Element<String, ()> = Element::Node("container".to_string(), vec![text]);
/// ```
pub enum Element<Node, Error>
where
    Node: From<String>,
{
    Component(Arc<dyn AnyComponent<Node = Node, Error = Error> + Send + Sync + 'static>),
    Node(Node, Vec<Element<Node, Error>>),
    Fragment(Vec<Element<Node, Error>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<Element<Node, Error>>),
    /// Identifies the element among its siblings, see [keyed].
    Keyed(Key, Box<Element<Node, Error>>),
    Suspense(Box<Element<Node, Error>>, Vec<Element<Node, Error>>),
}

impl<N, E> Element<N, E>
where
    N: From<String>,
{
    pub fn fragment(children: Vec<Element<N, E>>) -> Self {
        Self::Fragment(children)
    }
}

/// A keyed element keeps its state and native nodes when it is moved within a list,
/// and is rebuilt from scratch when its key changes.
/// ```
/// use bloom_core::{keyed, Element};
/// let item: Element<String, ()> = keyed(1, "one");
/// let list: Element<String, ()> = vec![item, keyed(2, "two")].into();
/// ```
pub fn keyed<N, E, K, T>(key: K, element: T) -> Element<N, E>
where
    N: From<String>,
    K: Hash,
    T: Into<Element<N, E>>,
{
    Element::Keyed(Key::new(key), Box::new(element.into()))
}

impl<N, E> From<Vec<Element<N, E>>> for Element<N, E>
where
    N: From<String>,
{
    fn from(children: Vec<Element<N, E>>) -> Self {
        Element::Fragment(children)
    }
}

impl<N, E> From<String> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: String) -> Self {
        Element::Node(N::from(value), vec![])
    }
}

impl<N, E> From<&str> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: &str) -> Self {
        Element::Node(N::from(value.to_string()), vec![])
    }
}

impl<N, E> From<&String> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: &String) -> Self {
        Element::Node(N::from(value.to_string()), vec![])
    }
}

impl<N, E> From<()> for Element<N, E>
where
    N: From<String>,
{
    fn from(_: ()) -> Self {
        Element::Fragment(Vec::new())
    }
}

impl<N, E> PartialEq for Element<N, E>
where
    N: From<String> + PartialEq + 'static,
    E: 'static,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Element::Component(a), Element::Component(b)) => a.compare(b) == ComponentDiff::Equal,
            (Element::Node(a, ac), Element::Node(b, bc)) => a == b && ac == bc,
            (Element::Fragment(ac), Element::Fragment(bc)) => ac == bc,
            (Element::Provider(av, ac), Element::Provider(bv, bc)) => {
                Arc::ptr_eq(av, bv) && ac == bc
            }
            (Element::Keyed(ak, a), Element::Keyed(bk, b)) => ak == bk && a == b,
            (Element::Suspense(af, ac), Element::Suspense(bf, bc)) => af == bf && ac == bc,
            _ => false,
        }
    }
}

impl<N, E> Clone for Element<N, E>
where
    N: From<String> + Clone,
{
    fn clone(&self) -> Self {
        match self {
            Element::Component(component) => Element::Component(component.clone()),
            Element::Fragment(children) => Element::Fragment(children.clone()),
            Element::Node(node, children) => Element::Node(node.clone(), children.clone()),
            Element::Provider(value, children) => {
                Element::Provider(value.clone(), children.clone())
            }
            Element::Keyed(key, element) => Element::Keyed(*key, element.clone()),
            Element::Suspense(fallback, children) => {
                Element::Suspense(fallback.clone(), children.clone())
            }
        }
    }
}
