use std::{any::Any, sync::Arc};

use crate::component::AnyComponent;

pub enum Element<Node, Error>
where
    Node: From<String>,
{
    Component(Arc<dyn AnyComponent<Node = Node, Error = Error> + Send + Sync + 'static>),
    Node(Node, Vec<Element<Node, Error>>),
    Fragment(Vec<Element<Node, Error>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<Element<Node, Error>>),
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
