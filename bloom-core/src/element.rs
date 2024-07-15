use std::{any::Any, sync::Arc};

use crate::component::{AnyComponent, ComponentDiff};

/// The element type is returned from component render-functions.
/// It can be constructed from a Node-type, e.G. HtmlNode, or a Component.
/// ```
/// HtmlNode::element("div").attr("id", "test123").build().into()
/// ```
pub enum Element<Node, Error>
where
    Node: From<String>,
{
    Component(Arc<dyn AnyComponent<Node = Node, Error = Error> + Send + Sync + 'static>),
    Node(Node, Vec<Element<Node, Error>>),
    Fragment(Vec<Element<Node, Error>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<Element<Node, Error>>),
}

impl<N, E> Element<N, E>
where
    N: From<String>,
{
    pub fn fragment(children: Vec<Element<N, E>>) -> Self {
        Self::Fragment(children)
    }
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
        }
    }
}
