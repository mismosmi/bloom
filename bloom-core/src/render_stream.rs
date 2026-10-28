use std::{
    any::{Any, TypeId},
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use async_context::provide_async_context;
use futures_util::{
    future::{self},
    stream::{self, once, FuturesOrdered},
    task::Spawn,
    Future, Stream, StreamExt,
};

use crate::{hook::Hook, CaughtError, Element};

use pin_project::pin_project;

type NodeStreamItem<N, E> = Result<(N, NodeStream<N, E>), E>;

#[pin_project]
pub struct NodeStream<N, E>(#[pin] Pin<Box<dyn Stream<Item = NodeStreamItem<N, E>> + Send>>);

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
    E: Send + 'static,
    N: Send + 'static,
{
    fn from(stream: impl Stream<Item = NodeStreamItem<N, E>> + Send + 'static) -> Self {
        Self(Box::pin(stream))
    }

    fn ready(item: NodeStreamItem<N, E>) -> Self {
        Self::from(once(future::ready(item)))
    }

    fn wrap(inner: impl Future<Output = NodeStream<N, E>> + Send + 'static) -> Self {
        Self(Box::pin(once(inner).flatten()))
    }
}

#[derive(Clone)]
struct RenderContext {
    context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl RenderContext {
    fn new() -> Self {
        Self {
            context: Arc::new(HashMap::new()),
        }
    }

    fn with_context(&self, value: Arc<dyn Any + Send + Sync>) -> Self {
        let mut new_context = self.context.as_ref().clone();
        new_context.insert((*value).type_id(), value);
        Self {
            context: Arc::new(new_context),
        }
    }
}

fn render_element<N, E, S>(
    element: Element<N, E>,
    spawner: S,
    ctx: RenderContext,
) -> Pin<Box<dyn Future<Output = NodeStream<N, E>> + Send>>
where
    N: From<String> + Send + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    match element {
        Element::Component(component) => Box::pin(async move {
            match provide_async_context(Hook::from_context(ctx.context.clone()), component.render())
                .await
            {
                (Ok(element), _) => render_element(element, spawner, ctx).await,
                (Err(error), _) => NodeStream::ready(Err(error)),
            }
        }),
        Element::Node(node, children) => Box::pin(future::ready(NodeStream::ready(Ok((
            node,
            render_children(children, spawner, ctx),
        ))))),
        Element::Fragment(children) => {
            Box::pin(future::ready(render_children(children, spawner, ctx)))
        }
        Element::Provider(provider, children) => Box::pin(future::ready(render_children(
            children,
            spawner,
            ctx.with_context(provider),
        ))),
        Element::Keyed(_, element) => render_element(*element, spawner, ctx),
        // Server-side rendering waits for all children anyway, so the fallback is never needed
        Element::Suspense(_, children) => {
            Box::pin(future::ready(render_children(children, spawner, ctx)))
        }
        // The children are buffered, so they can be replaced by the fallback if one of them fails.
        // They are rendered once, so the error can't be dismissed.
        Element::ErrorBoundary(fallback, children) => Box::pin(async move {
            match buffer(render_children(children, spawner.clone(), ctx.clone())).await {
                Ok(nodes) => replay(nodes),
                Err(error) => {
                    render_element(fallback(CaughtError::new(error, None)), spawner, ctx).await
                }
            }
        }),
    }
}

/// Rendered nodes with their rendered children
struct Buffered<N>(Vec<(N, Buffered<N>)>);

fn buffer<N, E>(
    mut nodes: NodeStream<N, E>,
) -> Pin<Box<dyn Future<Output = Result<Buffered<N>, E>> + Send>>
where
    N: Send + 'static,
    E: Send + 'static,
{
    Box::pin(async move {
        let mut buffered = Vec::new();
        while let Some(item) = nodes.next().await {
            let (node, children) = item?;
            buffered.push((node, buffer(children).await?));
        }
        Ok(Buffered(buffered))
    })
}

fn replay<N, E>(nodes: Buffered<N>) -> NodeStream<N, E>
where
    N: Send + 'static,
    E: Send + 'static,
{
    NodeStream::from(stream::iter(
        nodes
            .0
            .into_iter()
            .map(|(node, children)| Ok((node, replay(children)))),
    ))
}

fn render_children<N, E, S>(
    children: Vec<Element<N, E>>,
    spawner: S,
    ctx: RenderContext,
) -> NodeStream<N, E>
where
    N: From<String> + Send + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    let children = children
        .into_iter()
        .map(|child| render_element(child, spawner.clone(), ctx.clone()))
        .collect::<FuturesOrdered<_>>()
        .flatten();

    NodeStream::from(children)
}

/// render_stream is the main way to render some bloom-based UI once.
/// It takes an element and a spawner and returns a stream of nodes.
/// Libraries like bloom-server use this to render the UI to
/// a stream of serialized HTML to implement server-side rendering.
pub fn render_stream<N, E, S>(element: Element<N, E>, spawner: S) -> NodeStream<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    NodeStream::wrap(render_element(
        element,
        spawner.clone(),
        RenderContext::new(),
    ))
}
