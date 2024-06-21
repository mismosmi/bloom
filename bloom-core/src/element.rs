use std::sync::Arc;

use crate::component::AnyComponent;

pub enum Element<Node, Error> {
    Component(Arc<dyn AnyComponent<Node = Node, Error = Error> + Send + Sync + 'static>),
    Node(Node, Vec<Element<Node, Error>>),
    Fragment(Vec<Element<Node, Error>>),
}
