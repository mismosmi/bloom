use std::{any::Any, sync::Arc};

use crate::Element;
use async_trait::async_trait;

/// The component trait is the core of the bloom library.
/// It represents the basic unit of a bloom application.
#[async_trait]
pub trait Component: PartialEq<Self> + Send + Sync {
    /// The node type is a representation of the native UI elements of the target renderer.
    type Node: From<String>;
    /// The error type for the entire application.
    type Error;
    /// Components are roughly equivalent to React components.
    /// The struct itself represents the props of the component.
    /// The trait has a render method which contains the component logic.
    /// It should return the [Element] type, which is easily generated
    /// using the rsx macro from the bloom-rsx crate.
    /// Within the render function, hooks can be used such as [crate::use_state] and [crate::use_effect].
    /// ```
    /// use std::sync::Arc;
    ///
    /// use async_trait::async_trait;
    /// use bloom_core::{use_state, Component, Element};
    /// use bloom_html::{prelude::*, HtmlNode};
    /// use bloom_rsx::rsx;
    ///
    /// #[derive(PartialEq, Debug)]
    /// struct Counter {
    ///     initial_count: i32,
    /// }
    ///
    /// #[async_trait]
    /// impl Component for Counter {
    ///     type Node = HtmlNode;
    ///     type Error = ();
    ///
    ///     async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
    ///         let count = use_state(|| self.initial_count);
    ///
    ///         Ok(rsx!(
    ///             <div>{count.to_string()}</div>
    ///             <button on_click={move |_| count.update(|count| *count + 1)}>"Increment"</button>
    ///         ))
    ///     }
    /// }
    ///
    /// # let pool = futures_executor::ThreadPool::new().unwrap();
    /// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
    /// #     Counter { initial_count: 5 }.into(),
    /// #     pool,
    /// # ));
    /// # assert_eq!(html, Ok("<div>5</div><button>Increment</button>".to_string()));
    /// ```
    ///
    /// Components should usually implement a builder pattern for construction using the bloom-rsx macro.
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
