use std::{any::Any, sync::Arc};

use crate::Element;
use async_trait::async_trait;

/// The component trait is the core of the blom library.
/// Components are roughly equivalent to React components.
/// The struct itself represents the props of the component.
/// The trait has a render method which contains the component logic.
/// It should return the [Element] type, which is easily generated
/// using the rsx macro from the bloom-rsx crate.
/// Within the render function, hooks can be used such as [use_state] and [use_effect].
/// ```
/// use bloom_core::Component;
///
/// #[derive(PartialEq, Debug)]
/// struct Counter {
///     initial_count: i32
/// }
///
/// #[async_trait]
/// impl Component for Counter {
///   type Node = HtmlNode;
///   type Error = ();
///
///   async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
///     let count = use_state(|| self.initial_count);
///
///     rsx!(
///       <div>{count}</div>
///       <button on_click={move |_| count.update(|count| *count + 1)}>Increment</button>
///     )
/// ```
///
/// Components should usually implement a builder pattern for construction using the bloom-rsx macro.
#[async_trait]
pub trait Component: PartialEq<Self> + Send + Sync {
    type Node: From<String>;
    type Error;
    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error>;
}

impl<N, E, C> From<C> for Element<N, E>
where
    N: From<String>,
    C: Component<Node = N, Error = E> + Sized + 'static,
{
    fn from(component: C) -> Self {
        Element::Component(Arc::new(component))
    }
}

#[derive(PartialEq)]
pub enum ComponentDiff {
    NewType,
    NewProps,
    Equal,
}

#[async_trait]
pub trait AnyComponent {
    type Node: From<String>;
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

impl<N, E> PartialEq for &(dyn AnyComponent<Node = N, Error = E> + 'static)
where
    N: From<String>,
{
    fn eq(&self, other: &Self) -> bool {
        self.compare(other.as_any()) == ComponentDiff::Equal
    }
}
