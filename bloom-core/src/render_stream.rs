use std::{pin::Pin, sync::Arc, task::Poll};

use futures_util::{
    future::{self},
    stream::{once, FuturesOrdered},
    task::Spawn,
    Future, Stream, StreamExt,
};

use crate::{Component, Element, Node};

use pin_project::pin_project;

type NodeStreamItem<N, E> = Result<(N, NodeStream<N, E>), E>;

#[pin_project]
pub struct NodeStream<N, E>(#[pin] Pin<Box<dyn Stream<Item = NodeStreamItem<N, E>>>>);

impl<N, E> Stream for NodeStream<N, E> {
    type Item = NodeStreamItem<N, E>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let projection = self.project();
        projection.0.poll_next(cx)
    }
}

impl<N, E> NodeStream<N, E>
where
    E: 'static,
    N: 'static,
{
    fn from(stream: impl Stream<Item = NodeStreamItem<N, E>> + 'static) -> Self {
        Self(Box::pin(stream))
    }

    fn ready(item: NodeStreamItem<N, E>) -> Self {
        Self::from(once(future::ready(item)))
    }

    fn wrap(inner: impl Future<Output = NodeStream<N, E>> + 'static) -> Self {
        Self(Box::pin(once(inner).flatten()))
    }
}

fn render_element<N, E, S>(
    element: Element<N, E>,
    spawner: S,
) -> Pin<Box<dyn Future<Output = NodeStream<N, E>>>>
where
    N: Send + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    match element {
        Element::Component(component) => Box::pin(async move {
            match component.render().await {
                Ok(element) => render_element(element, spawner).await,
                Err(error) => NodeStream::ready(Err(error)),
            }
        }),
        Element::Node(node, children) => Box::pin(future::ready(NodeStream::ready(Ok((
            node,
            render_children(children, spawner),
        ))))),
        Element::Fragment(children) => Box::pin(future::ready(render_children(children, spawner))),
    }
}

fn render_children<N, E, S>(children: Vec<Element<N, E>>, spawner: S) -> NodeStream<N, E>
where
    N: Send + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    let children = children
        .into_iter()
        .map(|child| render_element(child, spawner.clone()))
        .collect::<FuturesOrdered<_>>()
        .flatten();

    NodeStream::from(children)
}

pub fn render_stream<N, E, C, S>(component: C, spawner: S) -> NodeStream<N, E>
where
    N: Node + Send + Sync + 'static,
    E: Send + 'static,
    C: Component<Node = N, Error = E> + 'static,
    S: Spawn + Clone + Send + 'static,
{
    NodeStream::wrap(render_element(
        Element::Component(Arc::new(component)),
        spawner.clone(),
    ))
}
