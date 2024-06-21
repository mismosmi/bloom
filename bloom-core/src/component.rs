use std::{any::Any, sync::Arc};

use crate::Element;
use async_trait::async_trait;

#[async_trait]
pub trait Component: PartialEq<Self> + Send + Sync {
    type Node;
    type Error;
    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error>;
}

#[derive(PartialEq)]
pub enum ComponentDiff {
    NewType,
    NewProps,
    Equal,
}

#[async_trait]
pub trait AnyComponent {
    type Node;
    type Error;

    fn compare(&self, other: &dyn Any) -> ComponentDiff;
    fn as_any(&self) -> &dyn Any;
    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error>;
}

#[async_trait]
impl<C> AnyComponent for C
where
    C: Component + 'static,
    Self: Sized,
{
    type Node = C::Node;
    type Error = C::Error;

    fn compare(&self, other: &dyn Any) -> ComponentDiff {
        let this = self;
        if let Some(other) = other.downcast_ref::<C>() {
            if this == other {
                return ComponentDiff::Equal;
            } else {
                return ComponentDiff::NewProps;
            }
        }
        ComponentDiff::NewType
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        Component::render(self).await
    }
}

impl<N, E> PartialEq for &(dyn AnyComponent<Node = N, Error = E> + 'static) {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other.as_any()) == ComponentDiff::Equal
    }
}
