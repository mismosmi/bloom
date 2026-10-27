use std::{any::Any, collections::HashMap, mem, pin::Pin, sync::Arc};

use async_channel::{bounded, unbounded, Receiver, Sender};
use futures_util::{
    future,
    task::{Spawn, SpawnExt},
    Future,
};

use crate::{
    component::{AnyComponent, ComponentDiff},
    context::ContextMap,
    effect::Effect,
    hook::Hook,
    render_queue::{EffectSlot, RenderContext, RenderQueue, RenderQueueItem},
    state::StateUpdate,
    suspense::{run_or_suspend, RunOrSuspendResult},
    Element, Key,
};

type RenderFuture<N, E> = Pin<Box<dyn Future<Output = (Result<Element<N, E>, E>, Hook)> + Send>>;

pub(crate) struct TreeComponent<N, E>
where
    N: From<String>,
{
    component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
    updates: Receiver<StateUpdate>,
    updater: Sender<StateUpdate>,
    render_result: Option<RenderFuture<N, E>>,
    child: Option<Box<TreeNode<N, E>>>,
    refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
    effects: EffectSlot,
    /// Effects of a detached component only run once its nodes are shown
    deferred_effects: Option<Vec<(u64, Effect)>>,
}

impl<N, E> TreeComponent<N, E>
where
    N: From<String>,
{
    fn new(component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>) -> Self {
        let (update_sender, update_receiver) = unbounded::<StateUpdate>();
        Self {
            component,
            state: HashMap::new(),
            updates: update_receiver,
            updater: update_sender,
            child: None,
            render_result: None,
            refs: HashMap::new(),
            effects: EffectSlot::default(),
            deferred_effects: None,
        }
    }

    fn queue_effects(
        &mut self,
        effects: Vec<(u64, Effect)>,
        render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
        ctx: &RenderContext<N>,
    ) {
        if ctx.detached {
            self.deferred_effects = Some(effects);
        } else {
            render_queue.queue_effects(&self.effects, effects);
        }
    }
}

pub(crate) enum SuspenseState<N, E>
where
    N: From<String>,
{
    /// The boundary was just created and does not know yet whether its children suspend
    Unsettled(Element<N, E>),
    /// Some children are still rendering, the fallback is shown in their place
    Fallback(Box<TreeNode<N, E>>),
    /// All children have rendered once and are shown
    Revealed,
}

pub(crate) struct TreeSuspense<N, E>
where
    N: From<String>,
{
    state: SuspenseState<N, E>,
    children: Vec<TreeNode<N, E>>,
}

impl<N, E> TreeSuspense<N, E>
where
    N: From<String>,
{
    fn is_revealed(&self) -> bool {
        matches!(self.state, SuspenseState::Revealed)
    }

    /// Children are only passed to the object model once the boundary is revealed
    fn children_ctx(&self, ctx: &RenderContext<N>) -> RenderContext<N> {
        if self.is_revealed() {
            ctx.clone()
        } else {
            ctx.detach()
        }
    }
}

pub(crate) enum TreeNode<N, E>
where
    N: From<String>,
{
    Component(TreeComponent<N, E>),
    Node(Arc<N>, Vec<TreeNode<N, E>>),
    Fragment(Vec<TreeNode<N, E>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<TreeNode<N, E>>),
    Keyed(Key, Box<TreeNode<N, E>>),
    Suspense(TreeSuspense<N, E>),
}

impl<N, E> TreeNode<N, E>
where
    N: From<String>,
{
    fn from(element: Element<N, E>) -> Self {
        match element {
            Element::Component(component) => TreeNode::Component(TreeComponent::new(component)),
            Element::Node(node, children) => TreeNode::Node(
                Arc::new(node),
                children
                    .into_iter()
                    .map(|child| TreeNode::from(child))
                    .collect(),
            ),
            Element::Fragment(children) => TreeNode::Fragment(
                children
                    .into_iter()
                    .map(|child| TreeNode::from(child))
                    .collect(),
            ),
            Element::Provider(value, children) => {
                TreeNode::Provider(value, children.into_iter().map(TreeNode::from).collect())
            }
            Element::Keyed(key, element) => {
                TreeNode::Keyed(key, Box::new(TreeNode::from(*element)))
            }
            Element::Suspense(fallback, children) => TreeNode::Suspense(TreeSuspense {
                state: SuspenseState::Unsettled(*fallback),
                children: children.into_iter().map(TreeNode::from).collect(),
            }),
        }
    }

    fn get_first_node(&self) -> Option<Arc<N>> {
        match self {
            Self::Component(component) => component
                .child
                .as_ref()
                .and_then(|child| child.get_first_node()),
            Self::Node(node, _) => Some(Arc::clone(node)),
            Self::Fragment(children) => {
                for child in children {
                    if let Some(node) = child.get_first_node() {
                        return Some(node);
                    }
                }
                None
            }
            Self::Provider(_, children) => {
                for child in children {
                    if let Some(node) = child.get_first_node() {
                        return Some(node);
                    }
                }
                None
            }
            Self::Keyed(_, child) => child.get_first_node(),
            Self::Suspense(suspense) => match &suspense.state {
                SuspenseState::Unsettled(_) => None,
                SuspenseState::Fallback(fallback) => fallback.get_first_node(),
                SuspenseState::Revealed => {
                    for child in &suspense.children {
                        if let Some(node) = child.get_first_node() {
                            return Some(node);
                        }
                    }
                    None
                }
            },
        }
    }

    /// Collects the outermost native nodes of this subtree in document order.
    fn collect_nodes(&self, nodes: &mut Vec<Arc<N>>) {
        match self {
            Self::Component(component) => {
                if let Some(child) = &component.child {
                    child.collect_nodes(nodes);
                }
            }
            Self::Node(node, _) => nodes.push(Arc::clone(node)),
            Self::Fragment(children) | Self::Provider(_, children) => {
                for child in children {
                    child.collect_nodes(nodes);
                }
            }
            Self::Keyed(_, child) => child.collect_nodes(nodes),
            Self::Suspense(suspense) => match &suspense.state {
                SuspenseState::Unsettled(_) => {}
                SuspenseState::Fallback(fallback) => fallback.collect_nodes(nodes),
                SuspenseState::Revealed => {
                    for child in &suspense.children {
                        child.collect_nodes(nodes);
                    }
                }
            },
        }
    }

    /// Whether a suspense boundary above this node has to keep waiting for it.
    /// Nested boundaries handle their own children, only their fallback counts.
    fn is_pending(&self) -> bool {
        match self {
            Self::Component(component) => {
                component.render_result.is_some()
                    || component
                        .child
                        .as_ref()
                        .is_some_and(|child| child.is_pending())
            }
            Self::Node(_, children) | Self::Fragment(children) | Self::Provider(_, children) => {
                children.iter().any(TreeNode::is_pending)
            }
            Self::Keyed(_, child) => child.is_pending(),
            Self::Suspense(suspense) => match &suspense.state {
                SuspenseState::Fallback(fallback) => fallback.is_pending(),
                SuspenseState::Unsettled(_) | SuspenseState::Revealed => false,
            },
        }
    }

    /// Passes a detached subtree to the object model, inserting it before the sibling.
    /// Effects that were held back while it was detached are queued.
    fn mount<P>(
        &mut self,
        parent: &Arc<N>,
        sibling: &Option<Arc<N>>,
        object_model: &mut P,
        render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    ) where
        P: ObjectModel<Node = N>,
    {
        match self {
            Self::Component(component) => {
                if let Some(effects) = component.deferred_effects.take() {
                    render_queue.queue_effects(&component.effects, effects);
                }
                if let Some(child) = component.child.as_mut() {
                    child.mount(parent, sibling, object_model, render_queue);
                }
            }
            Self::Node(node, children) => {
                object_model.create(node, parent, sibling);
                for child in children {
                    child.mount(node, &None, object_model, render_queue);
                }
            }
            Self::Fragment(children) | Self::Provider(_, children) => {
                for child in children {
                    child.mount(parent, sibling, object_model, render_queue);
                }
            }
            Self::Keyed(_, child) => child.mount(parent, sibling, object_model, render_queue),
            Self::Suspense(suspense) => match &mut suspense.state {
                SuspenseState::Unsettled(_) => {}
                SuspenseState::Fallback(fallback) => {
                    fallback.mount(parent, sibling, object_model, render_queue)
                }
                SuspenseState::Revealed => {
                    for child in &mut suspense.children {
                        child.mount(parent, sibling, object_model, render_queue);
                    }
                }
            },
        }
    }

    fn key(&self) -> Option<Key> {
        match self {
            Self::Keyed(key, _) => Some(*key),
            _ => None,
        }
    }
}

fn element_key<N, E>(element: &Element<N, E>) -> Option<Key>
where
    N: From<String>,
{
    match element {
        Element::Keyed(key, _) => Some(*key),
        _ => None,
    }
}

pub trait ObjectModel {
    type Node;
    fn start(&mut self) -> impl Future<Output = ()> + Send {
        // Do nothing by default
        future::ready(())
    }
    fn create(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    );
    fn remove(&mut self, node: &Arc<Self::Node>, parent: &Arc<Self::Node>);
    /// Moves an existing node within its parent so it is placed before the sibling,
    /// or at the end of the parent if there is no sibling.
    /// This is used to reorder keyed lists and must keep the node and its children intact.
    fn move_node(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    );
    fn update(&mut self, node: &Arc<Self::Node>, next: &Arc<Self::Node>);
    fn finalize(&mut self) -> impl Future<Output = ()> + Send {
        // Do nothing by default
        future::ready(())
    }
    fn subscribe(&mut self, _signal: Sender<()>) {
        // do nothing by default
    }
    fn get_context(&mut self) -> ContextMap {
        Arc::default()
    }
    fn set_context(&mut self, _ctx: ContextMap) {
        // do nothing by default
    }
}

/// render_loop can be used to implement interactive renderers on top of bloom-core.
/// The primary renderer is the bloom-client library which implements client side rendering
/// in the browser on top of bloom using webassembly.
/// The root is the root node in the host environment which will contain the rendered UI.
/// In web-environments the root might be a div within the body that is part of the server-sent html:
/// ```html
/// <html>
///   <body>
///     <div id="root"></div>
///   </body>
/// </html>
/// ```
///
///
/// The element is the root of the component tree that represents the intended UI.
/// This is usually a component.
///
/// The spawner-argument is
/// an object that implements the Spawn-trait so bloom can spawn async tasks on
/// arbitrary task runners such as tokio or async-std.
///
/// The future returned by render_loop is not `Send` because it keeps the cleanups of effects,
/// which can hold values that are not `Send` such as JavaScript objects.
/// Run it on a single-threaded executor, e.g. with `spawn_local`.
///
/// The object model is the interface to the host environment.
/// It is notified whenever a node needs to be created, removed or updated:
/// ```no_run
/// # use std::sync::Arc;
/// # use async_trait::async_trait;
/// # use bloom_core::{Component, Element};
/// use bloom_core::{render_loop, ObjectModel};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::{rsx, NoopBuilder};
///
/// struct PrintObjectModel;
///
/// impl ObjectModel for PrintObjectModel {
///     type Node = HtmlNode;
///
///     fn create(&mut self, node: &Arc<HtmlNode>, _parent: &Arc<HtmlNode>, _sibling: &Option<Arc<HtmlNode>>) {
///         println!("create {:?}", node);
///     }
///
///     fn move_node(&mut self, node: &Arc<HtmlNode>, _parent: &Arc<HtmlNode>, _sibling: &Option<Arc<HtmlNode>>) {
///         println!("Moved {:?}", node);
///     }
///
///     fn remove(&mut self, node: &Arc<HtmlNode>, _parent: &Arc<HtmlNode>) {
///         println!("remove {:?}", node);
///     }
///
///     fn update(&mut self, node: &Arc<HtmlNode>, next: &Arc<HtmlNode>) {
///         println!("update {:?} to {:?}", node, next);
///     }
/// }
///
/// # #[derive(PartialEq, NoopBuilder)]
/// # struct MyComponent;
/// # #[async_trait]
/// # impl Component for MyComponent {
/// #     type Node = HtmlNode;
/// #     type Error = ();
/// #     async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, ()>, ()> {
/// #         Ok(rsx!(<div />))
/// #     }
/// # }
/// let root = Arc::new(HtmlNode::element("div").build().into());
/// let spawner = futures_executor::ThreadPool::new().unwrap();
///
/// futures_executor::block_on(render_loop(root, rsx!(<MyComponent />), spawner, PrintObjectModel))
///     .unwrap();
/// ```
pub async fn render_loop<N, E, S, P>(
    root: Arc<N>,
    element: Element<N, E>,
    spawner: S,
    mut object_model: P,
) -> Result<(), E>
where
    N: From<String> + Send + 'static,
    E: Send + 'static,
    S: Spawn,
    P: ObjectModel<Node = N>,
{
    let mut tree_root = TreeNode::from(element);

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    object_model.subscribe(signal_sender.clone());

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    let mut created = false;

    while signal_receiver.recv().await.is_ok() {
        object_model.start().await;
        {
            let mut render_queue = RenderQueue::new();
            let ctx = RenderContext::new(root.clone(), None, object_model.get_context());
            // The first pass creates the whole tree, later passes only reload what changed
            if created {
                render_queue.reload(&mut tree_root, ctx);
            } else {
                render_queue.create(&mut tree_root, ctx);
                created = true;
            }

            while let Some(item) = render_queue.next() {
                match item {
                    RenderQueueItem::Create { current, ctx } => match unsafe { &mut *current } {
                        TreeNode::Component(component) => render_component(
                            component,
                            &mut render_queue,
                            &signal_sender,
                            ctx,
                            &spawner,
                        )?,
                        TreeNode::Node(node, children) => {
                            if !ctx.detached {
                                object_model.create(node, &ctx.parent, &ctx.sibling);
                            }
                            for child in children.iter_mut().rev() {
                                render_queue.create(child, ctx.with_parent(node.clone()));
                            }
                        }
                        // Children are created in order, so each one is inserted before the
                        // fragment's sibling, which already exists, rather than before the next child
                        TreeNode::Fragment(children) => {
                            for child in children.iter_mut().rev() {
                                render_queue.create(child, ctx.clone());
                            }
                        }
                        TreeNode::Provider(value, children) => {
                            for child in children.iter_mut().rev() {
                                render_queue.create(child, ctx.with_context(value.clone()));
                            }
                        }
                        TreeNode::Keyed(_, child) => render_queue.create(child.as_mut(), ctx),
                        // The fallback is only created once it is clear that the children suspend
                        TreeNode::Suspense(suspense) => {
                            render_queue.settle(unsafe { &mut *current }, ctx.clone());
                            let children_ctx = ctx.detach();
                            for child in suspense.children.iter_mut().rev() {
                                render_queue.create(child, children_ctx.clone());
                            }
                        }
                    },
                    RenderQueueItem::Reload { current, ctx } => match unsafe { &mut *current } {
                        TreeNode::Component(component) => {
                            if component.updates.is_empty() {
                                if let Some(render_result) = component.render_result.take() {
                                    match run_or_suspend(render_result) {
                                        RunOrSuspendResult::Suspend(render_result) => {
                                            component.render_result = Some(render_result);
                                            if let Some(ref mut child) = component.child {
                                                render_queue.reload(child.as_mut(), ctx);
                                            }
                                        }
                                        RunOrSuspendResult::Done((result, hook)) => {
                                            component.queue_effects(
                                                hook.effects,
                                                &mut render_queue,
                                                &ctx,
                                            );
                                            component.state = hook.state;
                                            component.refs = hook.refs;
                                            if let Some(ref mut child) = component.child {
                                                render_queue.update(child.as_mut(), result?, ctx);
                                            } else {
                                                let mut child = Box::new(TreeNode::from(result?));
                                                render_queue.create(child.as_mut(), ctx);
                                                component.child = Some(child);
                                            }
                                        }
                                    }
                                } else if let Some(ref mut child) = component.child {
                                    render_queue.reload(child.as_mut(), ctx);
                                } else {
                                    render_component(
                                        component,
                                        &mut render_queue,
                                        &signal_sender,
                                        ctx,
                                        &spawner,
                                    )?
                                }
                            } else {
                                render_component(
                                    component,
                                    &mut render_queue,
                                    &signal_sender,
                                    ctx,
                                    &spawner,
                                )?
                            }
                        }
                        TreeNode::Node(node, children) => {
                            let mut sibling = None;
                            for child in children.iter_mut().rev() {
                                render_queue.reload(
                                    child,
                                    ctx.with_parent_and_sibling(node.clone(), sibling),
                                );
                                sibling = child.get_first_node();
                            }
                        }
                        TreeNode::Fragment(children) => {
                            let mut sibling = ctx.sibling.clone();
                            for child in children.iter_mut().rev() {
                                render_queue.reload(child, ctx.with_sibling(sibling));
                                sibling = child.get_first_node();
                            }
                        }
                        TreeNode::Provider(value, children) => {
                            let mut sibling = ctx.sibling.clone();
                            for child in children.iter_mut().rev() {
                                render_queue.reload(
                                    child,
                                    ctx.with_sibling_and_context(sibling, value.clone()),
                                );
                                sibling = child.get_first_node();
                            }
                        }
                        TreeNode::Keyed(_, child) => render_queue.reload(child.as_mut(), ctx),
                        TreeNode::Suspense(suspense) => {
                            render_queue.settle(unsafe { &mut *current }, ctx.clone());
                            let children_ctx = suspense.children_ctx(&ctx);
                            if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                                render_queue.reload(fallback.as_mut(), ctx);
                            }
                            let mut sibling = children_ctx.sibling.clone();
                            for child in suspense.children.iter_mut().rev() {
                                render_queue.reload(child, children_ctx.with_sibling(sibling));
                                sibling = child.get_first_node();
                            }
                        }
                    },
                    RenderQueueItem::Update { current, next, ctx } => {
                        let current_node = unsafe { &mut *current };
                        // A changed key means a different element, so it is replaced instead of
                        // updated in place. Removing the old element runs its cleanups.
                        if current_node.key() != element_key(&next) {
                            replace_node(current_node, next, &mut render_queue, ctx);
                            continue;
                        }
                        match (current_node, next) {
                            (
                                TreeNode::Component(ref mut current_component),
                                Element::Component(next_component),
                            ) => match next_component.compare(current_component.component.as_any())
                            {
                                ComponentDiff::Equal => {
                                    render_queue.reload(unsafe { &mut *current }, ctx)
                                }
                                ComponentDiff::NewProps => {
                                    current_component.component = next_component;
                                    render_component(
                                        current_component,
                                        &mut render_queue,
                                        &signal_sender,
                                        ctx,
                                        &spawner,
                                    )?;
                                }
                                // Removing the old component runs its cleanups
                                ComponentDiff::NewType => {
                                    replace_node(
                                        unsafe { &mut *current },
                                        Element::Component(next_component),
                                        &mut render_queue,
                                        ctx,
                                    );
                                }
                            },
                            (
                                TreeNode::Node(current, current_children),
                                Element::Node(next, next_children),
                            ) => {
                                let next = Arc::new(next);
                                if !ctx.detached {
                                    object_model.update(current, &next);
                                }
                                *current = next.clone();
                                update_children(
                                    current_children,
                                    next_children,
                                    &mut render_queue,
                                    &mut object_model,
                                    ctx.with_parent(next),
                                );
                            }
                            (
                                TreeNode::Fragment(current_children),
                                Element::Fragment(next_children),
                            ) => update_children(
                                current_children,
                                next_children,
                                &mut render_queue,
                                &mut object_model,
                                ctx,
                            ),
                            (
                                TreeNode::Provider(_, current_children),
                                Element::Provider(next_value, next_children),
                            ) => update_children(
                                current_children,
                                next_children,
                                &mut render_queue,
                                &mut object_model,
                                ctx.with_context(next_value),
                            ),
                            // The keys are equal, they were compared above
                            (
                                TreeNode::Keyed(_, current_child),
                                Element::Keyed(_, next_element),
                            ) => render_queue.update(current_child.as_mut(), *next_element, ctx),
                            (
                                TreeNode::Suspense(suspense),
                                Element::Suspense(next_fallback, next_children),
                            ) => {
                                render_queue.settle(unsafe { &mut *current }, ctx.clone());
                                let children_ctx = suspense.children_ctx(&ctx);
                                // Once revealed, the fallback is never shown again
                                if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                                    render_queue.update(fallback.as_mut(), *next_fallback, ctx);
                                }
                                update_children(
                                    &mut suspense.children,
                                    next_children,
                                    &mut render_queue,
                                    &mut object_model,
                                    children_ctx,
                                );
                            }
                            (current_node, next) => {
                                replace_node(current_node, next, &mut render_queue, ctx)
                            }
                        }
                    }
                    RenderQueueItem::Settle { current, ctx } => {
                        let TreeNode::Suspense(suspense) = (unsafe { &mut *current }) else {
                            unreachable!("Only suspense boundaries are settled")
                        };
                        let pending = !suspense.is_revealed()
                            && suspense.children.iter().any(TreeNode::is_pending);
                        match std::mem::replace(&mut suspense.state, SuspenseState::Revealed) {
                            SuspenseState::Revealed => {}
                            SuspenseState::Unsettled(fallback) if pending => {
                                let mut fallback = Box::new(TreeNode::from(fallback));
                                render_queue.create(fallback.as_mut(), ctx);
                                suspense.state = SuspenseState::Fallback(fallback);
                            }
                            SuspenseState::Fallback(fallback) if pending => {
                                suspense.state = SuspenseState::Fallback(fallback);
                            }
                            previous => {
                                // A boundary inside a detached subtree is mounted along with it
                                if !ctx.detached {
                                    for child in &mut suspense.children {
                                        child.mount(
                                            &ctx.parent,
                                            &ctx.sibling,
                                            &mut object_model,
                                            &mut render_queue,
                                        );
                                    }
                                }
                                if let SuspenseState::Fallback(fallback) = previous {
                                    render_queue.remove(*fallback, ctx.attached_parent());
                                }
                            }
                        }
                    }
                    RenderQueueItem::Remove { current, parent } => match current {
                        TreeNode::Component(component) => {
                            render_queue.unmount(&component.effects);
                            if let Some(child) = component.child {
                                render_queue.remove(*child, parent);
                            }
                        }
                        TreeNode::Node(node, children) => {
                            if let Some(parent) = &parent {
                                object_model.remove(&node, parent);
                            }
                            for child in children {
                                render_queue
                                    .remove(child, parent.as_ref().map(|_| Arc::clone(&node)));
                            }
                        }
                        TreeNode::Fragment(children) | TreeNode::Provider(_, children) => {
                            for child in children {
                                render_queue.remove(child, parent.clone());
                            }
                        }
                        TreeNode::Keyed(_, child) => render_queue.remove(*child, parent),
                        TreeNode::Suspense(suspense) => {
                            let children_parent = if suspense.is_revealed() {
                                parent.clone()
                            } else {
                                None
                            };
                            if let SuspenseState::Fallback(fallback) = suspense.state {
                                render_queue.remove(*fallback, parent);
                            }
                            for child in suspense.children {
                                render_queue.remove(child, children_parent.clone());
                            }
                        }
                    },
                }
            }

            render_queue.run_effects();
        }
        object_model.finalize().await;
    }

    Ok(())
}

/// Children are matched by key if any of them has one and by position otherwise.
fn update_children<N, E, P>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    object_model: &mut P,
    ctx: RenderContext<N>,
) where
    N: From<String>,
    P: ObjectModel<Node = N>,
{
    if tree_nodes.iter().any(|node| node.key().is_some())
        || elements
            .iter()
            .any(|element| element_key(element).is_some())
    {
        update_keyed_children(tree_nodes, elements, render_queue, object_model, ctx);
    } else {
        update_children_by_position(tree_nodes, elements, render_queue, ctx);
    }
}

fn update_children_by_position<N, E>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    mut elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    ctx: RenderContext<N>,
) where
    N: From<String>,
{
    let old_len = tree_nodes.len();

    let kept = elements.len().min(old_len);
    for tree_node in tree_nodes.drain(kept..).rev() {
        render_queue.remove(tree_node, ctx.attached_parent());
    }

    tree_nodes.shrink_to_fit();

    for element in elements.drain(tree_nodes.len()..) {
        tree_nodes.push(TreeNode::from(element));
    }

    for tree_node in tree_nodes.iter_mut().skip(old_len).rev() {
        render_queue.create(tree_node, ctx.clone());
    }

    let mut sibling = ctx.sibling.clone();
    for (tree_node, element) in tree_nodes.iter_mut().zip(elements).rev() {
        let next_sibling = tree_node.get_first_node();
        render_queue.update(tree_node, element, ctx.with_sibling(sibling));
        sibling = next_sibling;
    }
}

/// Identifies a child in a keyed list.
/// Children without a key are identified by their position among the other unkeyed children.
#[derive(PartialEq, Eq, Hash)]
enum Slot {
    Key(Key),
    Index(usize),
}

fn slots(keys: impl Iterator<Item = Option<Key>>) -> impl Iterator<Item = Slot> {
    let mut index = 0;
    keys.map(move |key| match key {
        Some(key) => Slot::Key(key),
        None => {
            index += 1;
            Slot::Index(index - 1)
        }
    })
}

fn update_keyed_children<N, E, P>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    object_model: &mut P,
    ctx: RenderContext<N>,
) where
    N: From<String>,
    P: ObjectModel<Node = N>,
{
    let old_nodes = mem::take(tree_nodes);
    let mut old_indices = HashMap::with_capacity(old_nodes.len());
    for (index, slot) in slots(old_nodes.iter().map(TreeNode::key)).enumerate() {
        // Only the first of several children with the same key is matched
        old_indices.entry(slot).or_insert(index);
    }
    let mut old_nodes: Vec<Option<TreeNode<N, E>>> = old_nodes.into_iter().map(Some).collect();

    // For each child in the new order, the old index and next element of a matched child
    // or None for a new child
    let new_slots: Vec<Slot> = slots(elements.iter().map(element_key)).collect();
    let mut matches = Vec::with_capacity(elements.len());
    for (element, slot) in elements.into_iter().zip(new_slots) {
        match old_indices.remove(&slot) {
            Some(old_index) => {
                tree_nodes.push(old_nodes[old_index].take().expect("Child matched twice"));
                matches.push(Some((old_index, element)));
            }
            None => {
                tree_nodes.push(TreeNode::from(element));
                matches.push(None);
            }
        }
    }

    for old_node in old_nodes.into_iter().flatten().rev() {
        render_queue.remove(old_node, ctx.attached_parent());
    }

    // Matched children are moved right away so the new children and replaced nodes can be
    // inserted relative to them later.
    // Going from the end, every child that is part of the longest increasing subsequence
    // of old indices is already in place, every other one is moved before the next matched child.
    let old_order: Vec<usize> = matches
        .iter()
        .flatten()
        .map(|(old_index, _)| *old_index)
        .collect();
    let mut in_place = longest_increasing_subsequence(&old_order).into_iter().rev();
    let mut anchor = ctx.sibling.clone();
    let mut nodes = Vec::new();
    for (tree_node, _) in tree_nodes
        .iter()
        .zip(&matches)
        .rev()
        .filter(|(_, matched)| matched.is_some())
    {
        // Detached nodes are not in the object model yet, they are mounted in order later
        if !in_place.next().expect("Missing matched child") && !ctx.detached {
            nodes.clear();
            tree_node.collect_nodes(&mut nodes);
            for node in &nodes {
                object_model.move_node(node, &ctx.parent, &anchor);
            }
        }
        if let Some(first_node) = tree_node.get_first_node() {
            anchor = Some(first_node);
        }
    }

    // New children are not created yet, so only matched children can be siblings.
    // The queue processes children front to back, so new children that share a sibling
    // are still inserted in order.
    let mut sibling = ctx.sibling.clone();
    for (tree_node, matched) in tree_nodes.iter_mut().zip(matches).rev() {
        match matched {
            Some((_, element)) => {
                let first_node = tree_node.get_first_node();
                render_queue.update(tree_node, element, ctx.with_sibling(sibling.clone()));
                if first_node.is_some() {
                    sibling = first_node;
                }
            }
            None => render_queue.create(tree_node, ctx.with_sibling(sibling.clone())),
        }
    }
}

/// Marks the values that are part of a longest strictly increasing subsequence.
fn longest_increasing_subsequence(values: &[usize]) -> Vec<bool> {
    // tails[k] is the index of the smallest last value of an increasing subsequence of length k + 1
    let mut tails: Vec<usize> = Vec::new();
    let mut predecessors = vec![None; values.len()];
    for (index, value) in values.iter().enumerate() {
        let length = tails.partition_point(|&tail| values[tail] < *value);
        if length > 0 {
            predecessors[index] = Some(tails[length - 1]);
        }
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }

    let mut in_sequence = vec![false; values.len()];
    let mut current = tails.last().copied();
    while let Some(index) = current {
        in_sequence[index] = true;
        current = predecessors[index];
    }
    in_sequence
}

fn render_component<N, E, S>(
    tree_component: &mut TreeComponent<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    signal_sender: &Sender<()>,
    ctx: RenderContext<N>,
    spawner: &S,
) -> Result<(), E>
where
    N: From<String> + Send + 'static,
    E: Send + 'static,
    S: Spawn,
{
    while let Ok(state_update) = tree_component.updates.try_recv() {
        state_update.apply(&mut tree_component.state);
    }

    let component = Arc::clone(&tree_component.component);
    let hook = Hook::new(
        signal_sender.clone(),
        tree_component.updater.clone(),
        tree_component.state.clone(),
        tree_component.refs.clone(),
        ctx.context.clone(),
    );
    let result = run_or_suspend(Box::pin(async_context::provide_async_context(
        hook,
        component.render(),
    )));

    let _: () = match result {
        RunOrSuspendResult::Done((element, hook)) => {
            tree_component.render_result = None;
            match tree_component.child {
                Some(ref mut node) => render_queue.update(node.as_mut(), element?, ctx.clone()),
                None => {
                    let tree_node = TreeNode::from(element?);
                    let mut child = Box::new(tree_node);
                    render_queue.create(child.as_mut(), ctx.clone());
                    tree_component.child = Some(child);
                }
            }
            tree_component.queue_effects(hook.effects, render_queue, &ctx);
            tree_component.state = hook.state;
            tree_component.refs = hook.refs;
        }
        RunOrSuspendResult::Suspend(render_future) => {
            let signal_sender = signal_sender.clone();
            tree_component.render_result = Some(Box::pin(
                spawner
                    .spawn_with_handle(async move {
                        let result = render_future.await;
                        let _ = signal_sender.try_send(());
                        result
                    })
                    .expect("Failed to spawn async task"),
            ));
        }
    };
    Ok(())
}

fn replace_node<N, E>(
    node: &mut TreeNode<N, E>,
    element: Element<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    ctx: RenderContext<N>,
) where
    N: From<String>,
{
    let mut old_node = TreeNode::from(element);
    std::mem::swap(node, &mut old_node);
    render_queue.remove(old_node, ctx.attached_parent());
    render_queue.create(node, ctx);
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        hash::Hash,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use async_channel::{Receiver, RecvError, Sender};
    use async_trait::async_trait;
    use futures_util::{task::Spawn, Future, FutureExt};

    use crate::{
        keyed, use_effect, use_ref_with_default, use_state, Component, Element, Fragment,
        ObjectModel, Provider, Suspense,
    };

    fn node_id(node: &Arc<MockNode>) -> usize {
        Arc::as_ptr(node) as usize
    }

    struct InnerMockObjectModel {
        created: VecDeque<Arc<MockNode>>,
        existing: Vec<Arc<MockNode>>,
        updated: VecDeque<Arc<MockNode>>,
        removed: VecDeque<Arc<MockNode>>,
        moved: VecDeque<Arc<MockNode>>,
        // The children of each node in order
        children: HashMap<usize, Vec<Arc<MockNode>>>,
        start_signal: (Sender<()>, Receiver<()>),
        finalize_signal: (Sender<()>, Receiver<()>),
    }

    impl InnerMockObjectModel {
        fn new() -> Arc<Mutex<Self>> {
            Arc::new(Mutex::new(Self {
                created: VecDeque::new(),
                existing: Vec::new(),
                updated: VecDeque::new(),
                removed: VecDeque::new(),
                moved: VecDeque::new(),
                children: HashMap::new(),
                start_signal: async_channel::bounded(1),
                finalize_signal: async_channel::bounded(2),
            }))
        }

        fn assert_created(&mut self, expected: MockNode) {
            assert_eq!(
                &self.created.pop_front(),
                &Some(Arc::new(expected)),
                "Node not created"
            );
        }

        fn assert_updated(&mut self, expected: MockNode) {
            assert_eq!(
                &self.updated.pop_front(),
                &Some(Arc::new(expected)),
                "Node not updated"
            );
        }

        #[allow(dead_code)]
        fn assert_removed(&mut self, expected: MockNode) {
            assert_eq!(
                &self.removed.pop_front(),
                &Some(Arc::new(expected)),
                "Node not removed"
            );
        }

        fn assert_noop(&self) {
            assert!(self.created.is_empty());
            assert!(self.updated.is_empty());
            assert!(self.removed.is_empty());
            assert!(self.moved.is_empty());
        }

        fn children_of(&self, parent: &Arc<MockNode>) -> Vec<i32> {
            self.children
                .get(&node_id(parent))
                .map(|children| children.iter().map(|child| child.0).collect())
                .unwrap_or_default()
        }

        fn drain(queue: &mut VecDeque<Arc<MockNode>>) -> Vec<i32> {
            let mut values: Vec<i32> = queue.drain(..).map(|node| node.0).collect();
            values.sort();
            values
        }

        /// Checks the children of the parent and which nodes were created, removed and moved
        /// since the last check
        fn assert_children(
            &mut self,
            parent: &Arc<MockNode>,
            children: &[i32],
            created: &[i32],
            removed: &[i32],
            moved: usize,
        ) {
            assert_eq!(self.children_of(parent), children, "Wrong children");
            assert_eq!(
                Self::drain(&mut self.created),
                created,
                "Wrong nodes created"
            );
            assert_eq!(
                Self::drain(&mut self.removed),
                removed,
                "Wrong nodes removed"
            );
            assert_eq!(self.moved.drain(..).count(), moved, "Wrong number of moves");
            self.updated.clear();
        }

        fn insert(
            &mut self,
            node: &Arc<MockNode>,
            parent: &Arc<MockNode>,
            sibling: &Option<Arc<MockNode>>,
        ) {
            let children = self.children.entry(node_id(parent)).or_default();
            let index = match sibling {
                Some(sibling) => children
                    .iter()
                    .position(|child| Arc::ptr_eq(child, sibling))
                    .unwrap_or_else(|| panic!("Sibling {:?} not in parent", sibling)),
                None => children.len(),
            };
            children.insert(index, node.clone());
        }

        fn detach(&mut self, node: &Arc<MockNode>, parent: &Arc<MockNode>) {
            let children = self
                .children
                .get_mut(&node_id(parent))
                .expect("Parent has no children");
            let index = children
                .iter()
                .position(|child| Arc::ptr_eq(child, node))
                .unwrap_or_else(|| panic!("{:?} not in parent", node));
            children.remove(index);
        }

        fn render_cycle(&self) -> impl Future<Output = ()> {
            let start_signal = self.start_signal.1.clone();
            let finalize_signal = self.finalize_signal.1.clone();
            async move {
                start_signal.recv().await.unwrap();
                finalize_signal.recv().await.unwrap();
            }
        }
    }

    struct MockObjectModel(Arc<Mutex<InnerMockObjectModel>>);

    #[derive(Debug, PartialEq)]
    struct MockNode(i32);

    impl From<String> for MockNode {
        fn from(_value: String) -> Self {
            MockNode(0)
        }
    }

    impl ObjectModel for MockObjectModel {
        type Node = MockNode;
        fn create(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            parent: &std::sync::Arc<Self::Node>,
            sibling: &Option<std::sync::Arc<Self::Node>>,
        ) {
            println!("create {:?}", node);
            let mut inner = self.0.lock().unwrap();
            if let Some(sibling) = sibling {
                assert!(
                    inner.existing.iter().any(|node| Arc::ptr_eq(node, sibling)),
                    "{:?} created before its sibling {:?}",
                    node,
                    sibling
                );
            }
            inner.existing.push(node.clone());
            inner.created.push_back(node.clone());
            inner.insert(node, parent, sibling);
        }

        fn update(&mut self, node: &std::sync::Arc<Self::Node>, next: &std::sync::Arc<Self::Node>) {
            let mut inner = self.0.lock().unwrap();
            inner.existing.push(next.clone());
            inner.updated.push_back(next.clone());
            if let Some(children) = inner.children.remove(&node_id(node)) {
                inner.children.insert(node_id(next), children);
            }
            for children in inner.children.values_mut() {
                for child in children.iter_mut() {
                    if Arc::ptr_eq(child, node) {
                        *child = next.clone();
                    }
                }
            }
        }

        fn remove(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            parent: &std::sync::Arc<Self::Node>,
        ) {
            let mut inner = self.0.lock().unwrap();
            inner.detach(node, parent);
            inner.removed.push_back(node.clone());
        }

        fn move_node(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            parent: &std::sync::Arc<Self::Node>,
            sibling: &Option<std::sync::Arc<Self::Node>>,
        ) {
            let mut inner = self.0.lock().unwrap();
            inner.detach(node, parent);
            inner.insert(node, parent, sibling);
            inner.moved.push_back(node.clone());
        }

        async fn start(&mut self) {
            let signal = self.0.lock().unwrap().start_signal.0.clone();
            signal.send(()).await.unwrap();
        }

        async fn finalize(&mut self) {
            let signal = self.0.lock().unwrap().finalize_signal.0.clone();
            signal.send(()).await.unwrap();
        }
    }

    struct TokioSpawner;

    impl Spawn for TokioSpawner {
        fn spawn_obj(
            &self,
            future: futures_util::task::FutureObj<'static, ()>,
        ) -> Result<(), futures_util::task::SpawnError> {
            tokio::spawn(future.map(|_| ()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn render_basic_component() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct MockComponent;

                #[async_trait]
                impl Component for MockComponent {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        Ok(Element::Node(MockNode(0), Vec::new()))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(MockComponent));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_created(MockNode(0));

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn with_callback() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct AutoCounter;

                #[async_trait]
                impl Component for AutoCounter {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let counter = use_state(|| 0i32);
                        if *counter == 0 {
                            counter.update(|count| *count + 1);
                        }
                        Ok(Element::Node(MockNode(*counter), Vec::new()))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(AutoCounter));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_created(MockNode(0));

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_updated(MockNode(1));

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn state_keeps_initial_value() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct Child {
                    value: i32,
                }

                #[async_trait]
                impl Component for Child {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        // The initial value is only built once, later props don't change it
                        let first_value = use_state(|| self.value);
                        Ok(Element::Node(MockNode(*first_value), Vec::new()))
                    }
                }

                #[derive(PartialEq)]
                struct Parent;

                #[async_trait]
                impl Component for Parent {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let counter = use_state(|| 0i32);
                        if *counter == 0 {
                            counter.update(|count| *count + 1);
                        }
                        Ok(Element::Component(Arc::new(Child { value: *counter })))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(Parent));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_created(MockNode(0));

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_updated(MockNode(0));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn update_order() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct MultiContent;

                #[async_trait]
                impl Component for MultiContent {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let counter = use_state(|| 0i32);

                        if *counter == 0 {
                            let counter = counter.clone();
                            tokio::spawn(async move { counter.update(|count| *count + 1) });
                        }

                        Ok(Element::Node(
                            MockNode(*counter),
                            vec![
                                Element::Node(MockNode(3), Vec::new()),
                                Element::Node(MockNode(4), Vec::new()),
                                Element::Node(MockNode(5), Vec::new()),
                            ],
                        ))
                    }
                }
                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(MultiContent));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                println!("start first checks");
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(0));
                    lock.assert_created(MockNode(3));
                    lock.assert_created(MockNode(4));
                    lock.assert_created(MockNode(5));
                }
                println!("first cycle done");
                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_updated(MockNode(1));
                    lock.assert_updated(MockNode(3));
                    lock.assert_updated(MockNode(4));
                    lock.assert_updated(MockNode(5));
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn render_root_node() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let mut handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element: Element<MockNode, ()> = Element::fragment(vec![
                        Element::Node(MockNode(1), vec![Element::Node(MockNode(2), Vec::new())]),
                        Element::Node(MockNode(3), Vec::new()),
                    ]);
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                tokio::select! {
                    _ = render_cycle => {}
                    result = &mut handle => panic!("render loop stopped: {:?}", result),
                }
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(1));
                    lock.assert_created(MockNode(2));
                    lock.assert_created(MockNode(3));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn grow_children() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct GrowingList;

                #[async_trait]
                impl Component for GrowingList {
                    type Error = ();
                    type Node = MockNode;
                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let length = use_state(|| 1i32);

                        if *length < 3 {
                            let length = length.clone();
                            tokio::spawn(async move { length.update(|length| *length + 2) });
                        }

                        Ok(Element::Node(
                            MockNode(0),
                            (1..=*length)
                                .map(|i| Element::Node(MockNode(i), Vec::new()))
                                .collect(),
                        ))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let mut handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(GrowingList));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(0));
                    lock.assert_created(MockNode(1));
                    lock.assert_noop();
                }

                // Fail instead of waiting forever if the render loop panics
                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                tokio::select! {
                    _ = render_cycle => {}
                    result = &mut handle => panic!("render loop stopped: {:?}", result),
                }
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(2));
                    lock.assert_created(MockNode(3));
                    lock.assert_updated(MockNode(0));
                    lock.assert_updated(MockNode(1));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn async_component() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (sender, receiver) = async_channel::bounded::<()>(1);

                struct AsyncComponent(Receiver<()>);

                impl PartialEq for AsyncComponent {
                    fn eq(&self, _: &Self) -> bool {
                        true
                    }
                }

                #[async_trait]
                impl Component for AsyncComponent {
                    type Error = RecvError;
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        self.0.recv().await?;
                        Ok(Element::Node(MockNode(0), Vec::new()))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(AsyncComponent(receiver)));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;
                inner_object_model.lock().unwrap().assert_noop();

                sender.send(()).await.unwrap();
                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;

                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_created(MockNode(0));

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn with_effect() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (sender, receiver) = async_channel::bounded::<()>(1);

                #[derive(Clone)]
                struct MySender(Sender<()>);

                impl Hash for MySender {
                    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                        std::ptr::hash(&self.0 as *const Sender<()>, state);
                    }
                }

                impl PartialEq for MySender {
                    fn eq(&self, other: &Self) -> bool {
                        std::ptr::eq(&self.0, &other.0)
                    }
                }

                #[derive(PartialEq)]
                struct EffectComponent(MySender);

                #[async_trait]
                impl Component for EffectComponent {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        use_effect(self.0.clone(), |sender| {
                            sender.0.try_send(()).unwrap();
                        });
                        Ok(Element::Node(MockNode(0), Vec::new()))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(EffectComponent(MySender(sender))));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                render_cycle.await;

                assert_eq!(Ok(()), receiver.recv().await);

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn effect_lifecycle() {
        // The render loop isn't Send, so it runs on a LocalSet
        tokio::task::LocalSet::new()
            .run_until(async move {
                type Log = Arc<Mutex<Vec<String>>>;

                /// Effect argument, only the dependency is part of the hash
                #[derive(Clone)]
                struct Probe {
                    dependency: i32,
                    log: Log,
                }

                impl Hash for Probe {
                    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                        self.dependency.hash(state);
                    }
                }

                struct Child {
                    dependency: i32,
                    render: i32,
                    log: Log,
                }

                impl PartialEq for Child {
                    fn eq(&self, other: &Self) -> bool {
                        self.dependency == other.dependency && self.render == other.render
                    }
                }

                #[async_trait]
                impl Component for Child {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        use_effect(
                            Probe {
                                dependency: self.dependency,
                                log: self.log.clone(),
                            },
                            |probe| {
                                probe
                                    .log
                                    .lock()
                                    .unwrap()
                                    .push(format!("effect {}", probe.dependency));
                                // Cleanups don't have to be Send
                                let not_send = std::rc::Rc::new(probe.dependency);
                                move || {
                                    probe
                                        .log
                                        .lock()
                                        .unwrap()
                                        .push(format!("cleanup {}", not_send));
                                }
                            },
                        );
                        Ok(Element::Node(MockNode(self.render), Vec::new()))
                    }
                }

                struct Parent(Log);

                impl PartialEq for Parent {
                    fn eq(&self, _: &Self) -> bool {
                        true
                    }
                }

                #[async_trait]
                impl Component for Parent {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let step = use_state(|| 0i32);
                        if *step < 3 {
                            let step = step.clone();
                            tokio::spawn(async move { step.update(|step| *step + 1) });
                        }
                        // Same dependency, then a new one, then the child is removed
                        let (dependency, render) = match *step {
                            0 => (1, 0),
                            1 => (1, 1),
                            2 => (2, 2),
                            _ => return Ok(().into()),
                        };
                        Ok(Element::Component(Arc::new(Child {
                            dependency,
                            render,
                            log: self.0.clone(),
                        })))
                    }
                }

                let log = Log::default();
                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let parent_log = log.clone();
                let mut handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(Parent(parent_log)));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                for expected in [
                    vec!["effect 1"],
                    vec![],
                    vec!["cleanup 1", "effect 2"],
                    vec!["cleanup 2"],
                ] {
                    // Fail instead of waiting forever if the render loop panics
                    let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                    tokio::select! {
                        _ = render_cycle => {}
                        result = &mut handle => panic!("render loop stopped: {:?}", result),
                    }
                    let entries: Vec<String> = log.lock().unwrap().drain(..).collect();
                    assert_eq!(entries, expected);
                }

                handle.abort();
            })
            .await;
    }

    /// Renders the next list of ids on every render cycle
    struct Steps {
        steps: Vec<Vec<i32>>,
        render: fn(&[i32]) -> Element<MockNode, ()>,
    }

    impl PartialEq for Steps {
        fn eq(&self, _: &Self) -> bool {
            true
        }
    }

    #[async_trait]
    impl Component for Steps {
        type Error = ();
        type Node = MockNode;

        async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
            let step = use_state(|| 0usize);
            if *step + 1 < self.steps.len() {
                let step = step.clone();
                tokio::spawn(async move { step.update(|step| *step + 1) });
            }
            Ok((self.render)(&self.steps[*step]))
        }
    }

    struct StepsHarness {
        inner: Arc<Mutex<InnerMockObjectModel>>,
        root: Arc<MockNode>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl StepsHarness {
        fn start(steps: Vec<Vec<i32>>, render: fn(&[i32]) -> Element<MockNode, ()>) -> Self {
            let inner = InnerMockObjectModel::new();
            let object_model = MockObjectModel(inner.clone());
            let root = Arc::new(MockNode(0));
            let loop_root = root.clone();
            // The render loop isn't Send, so tests using the harness run on a LocalSet
            let handle = tokio::task::spawn_local(async move {
                let element = Element::Component(Arc::new(Steps { steps, render }));
                super::render_loop(loop_root, element, TokioSpawner, object_model)
                    .await
                    .unwrap();
            });
            Self {
                inner,
                root,
                handle,
            }
        }

        async fn next_cycle(&mut self) -> std::sync::MutexGuard<'_, InnerMockObjectModel> {
            // Fail instead of waiting forever if the render loop panics
            let render_cycle = self.inner.lock().unwrap().render_cycle();
            tokio::select! {
                _ = render_cycle => {}
                result = &mut self.handle => panic!("render loop stopped: {:?}", result),
            }
            self.inner.lock().unwrap()
        }
    }

    fn keyed_fragment(key: i32, children: Vec<Element<MockNode, ()>>) -> Element<MockNode, ()> {
        keyed(key, Fragment::builder().children(children).build())
    }

    fn keyed_nodes(ids: &[i32]) -> Element<MockNode, ()> {
        Element::fragment(
            ids.iter()
                .map(|id| keyed(*id, Element::Node(MockNode(*id), vec![])))
                .collect(),
        )
    }

    #[tokio::test]
    async fn reorder_keyed_nodes() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let mut harness = StepsHarness::start(
                    vec![
                        vec![1, 2, 3, 4, 5],
                        vec![5, 1, 2, 3, 4],
                        vec![5, 4, 3, 2, 1],
                        vec![2, 6, 4],
                        vec![7, 2, 8, 4, 9],
                        vec![],
                    ],
                    keyed_nodes,
                );
                let root = harness.root.clone();

                harness.next_cycle().await.assert_children(
                    &root,
                    &[1, 2, 3, 4, 5],
                    &[1, 2, 3, 4, 5],
                    &[],
                    0,
                );
                // Only the last item moves to the front
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[5, 1, 2, 3, 4], &[], &[], 1);
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[5, 4, 3, 2, 1], &[], &[], 3);
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[2, 6, 4], &[6], &[1, 3, 5], 1);
                harness.next_cycle().await.assert_children(
                    &root,
                    &[7, 2, 8, 4, 9],
                    &[7, 8, 9],
                    &[6],
                    0,
                );
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[], &[], &[2, 4, 7, 8, 9], 0);

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn reorder_keyed_providers() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let mut harness = StepsHarness::start(vec![vec![1, 2, 3], vec![3, 1, 2]], |ids| {
                    Element::fragment(
                        ids.iter()
                            .map(|id| {
                                keyed(
                                    *id,
                                    Provider::builder()
                                        .value(*id)
                                        .children(vec![Element::Node(MockNode(*id), vec![])])
                                        .build(),
                                )
                            })
                            .collect(),
                    )
                });
                let root = harness.root.clone();

                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[1, 2, 3], &[1, 2, 3], &[], 0);
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[3, 1, 2], &[], &[], 1);

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn mixed_keyed_and_unkeyed_children() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                // Positive ids are keyed, unkeyed children are matched by their position among each other
                let mut harness =
                    StepsHarness::start(vec![vec![-1, 1, 2], vec![2, -2, 1]], |ids| {
                        Element::fragment(
                            ids.iter()
                                .map(|id| {
                                    let node = Element::Node(MockNode(*id), vec![]);
                                    if *id > 0 {
                                        keyed_fragment(*id, vec![node])
                                    } else {
                                        node
                                    }
                                })
                                .collect(),
                        )
                    });
                let root = harness.root.clone();

                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[-1, 1, 2], &[-1, 1, 2], &[], 0);
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[2, -2, 1], &[], &[], 1);

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn changed_key_replaces_element() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let mut harness = StepsHarness::start(vec![vec![1], vec![2]], |ids| {
                    keyed_fragment(ids[0], vec![Element::Node(MockNode(0), vec![])])
                });
                let root = harness.root.clone();

                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[0], &[0], &[], 0);
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[0], &[0], &[0], 0);

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn keyed_components_keep_state() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct Item {
                    id: i32,
                }

                #[async_trait]
                impl Component for Item {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        // Shows the wrong id if the state ends up in another item
                        let first_id = use_state(|| self.id);
                        Ok(Element::Node(MockNode(*first_id), vec![]))
                    }
                }

                let mut harness = StepsHarness::start(vec![vec![1, 2, 3], vec![3, 1, 2]], |ids| {
                    Element::Node(
                        MockNode(100),
                        ids.iter()
                            .map(|id| keyed(*id, Element::Component(Arc::new(Item { id: *id }))))
                            .collect(),
                    )
                });
                let root = harness.root.clone();

                let list = {
                    let mut inner = harness.next_cycle().await;
                    let list = inner.children[&node_id(&root)][0].clone();
                    inner.assert_children(&list, &[1, 2, 3], &[1, 2, 3, 100], &[], 0);
                    list
                };
                let mut inner = harness.next_cycle().await;
                // The list node is updated, its children are kept under the new node
                let next_list = inner.children[&node_id(&root)][0].clone();
                assert!(!Arc::ptr_eq(&list, &next_list));
                inner.assert_children(&next_list, &[3, 1, 2], &[], &[], 1);
                drop(inner);

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn changed_key_resets_component_state() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(PartialEq)]
                struct Form {
                    id: i32,
                    value: i32,
                }

                #[async_trait]
                impl Component for Form {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        // Keeps the first value until the component is rebuilt
                        let first_value = use_ref_with_default(|| self.value);
                        Ok(Element::Node(MockNode(*first_value), vec![]))
                    }
                }

                // Each step renders a single form with [id, value]
                let mut harness =
                    StepsHarness::start(vec![vec![1, 10], vec![1, 11], vec![2, 20]], |step| {
                        keyed(
                            step[0],
                            Element::Component(Arc::new(Form {
                                id: step[0],
                                value: step[1],
                            })),
                        )
                    });
                let root = harness.root.clone();

                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[10], &[10], &[], 0);
                // Same key: the component is updated and keeps its state
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[10], &[], &[], 0);
                // New key: the component is rebuilt with fresh state
                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[20], &[20], &[10], 0);

                harness.handle.abort();
            })
            .await;
    }

    #[test]
    fn longest_increasing_subsequence() {
        assert_eq!(
            super::longest_increasing_subsequence(&[]),
            Vec::<bool>::new()
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[4, 0, 1, 2, 3]),
            vec![false, true, true, true, true]
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[0, 4, 3, 2, 1])
                .into_iter()
                .filter(|in_sequence| *in_sequence)
                .count(),
            2
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[2, 0, 3, 1, 4]),
            vec![false, true, false, true, true]
        );
    }
    /// Renders its node once the receiver gets a message
    struct Delayed(Receiver<()>, i32);

    impl PartialEq for Delayed {
        fn eq(&self, _: &Self) -> bool {
            true
        }
    }

    #[async_trait]
    impl Component for Delayed {
        type Error = RecvError;
        type Node = MockNode;

        async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
            self.0.recv().await?;
            Ok(Element::Node(MockNode(self.1), Vec::new()))
        }
    }

    fn delayed(receiver: &Receiver<()>, value: i32) -> Element<MockNode, RecvError> {
        Element::Component(Arc::new(Delayed(receiver.clone(), value)))
    }

    fn node(value: i32) -> Element<MockNode, RecvError> {
        Element::Node(MockNode(value), Vec::new())
    }

    fn start_render_loop(
        element: Element<MockNode, RecvError>,
    ) -> (
        Arc<Mutex<InnerMockObjectModel>>,
        tokio::task::JoinHandle<()>,
    ) {
        let inner_object_model = InnerMockObjectModel::new();
        let object_model = MockObjectModel(inner_object_model.clone());
        // The render loop isn't Send, so it runs on a LocalSet
        let handle = tokio::task::spawn_local(async move {
            let root = Arc::new(MockNode(0));
            super::render_loop(root, element, TokioSpawner, object_model)
                .await
                .unwrap();
        });
        (inner_object_model, handle)
    }

    async fn next_render_cycle(
        inner_object_model: &Arc<Mutex<InnerMockObjectModel>>,
        handle: &mut tokio::task::JoinHandle<()>,
    ) {
        let render_cycle = inner_object_model.lock().unwrap().render_cycle();
        tokio::select! {
            _ = render_cycle => {}
            result = handle => panic!("render loop stopped: {:?}", result),
        }
    }

    #[tokio::test]
    async fn suspense_shows_fallback() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (sender, receiver) = async_channel::bounded::<()>(1);

                let element = Element::Fragment(vec![
                    Suspense::builder()
                        .fallback(node(9))
                        .children(vec![node(1), delayed(&receiver, 2), node(3)])
                        .build()
                        .into(),
                    node(4),
                ]);
                let (inner_object_model, mut handle) = start_render_loop(element);

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(9));
                    lock.assert_created(MockNode(4));
                    lock.assert_noop();
                }

                sender.send(()).await.unwrap();
                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(1));
                    lock.assert_created(MockNode(2));
                    lock.assert_created(MockNode(3));
                    lock.assert_removed(MockNode(9));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn suspense_without_suspending_children() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let element = Suspense::builder()
                    .fallback(node(9))
                    .children(vec![node(1), Element::Node(MockNode(2), vec![node(3)])])
                    .build()
                    .into();
                let (inner_object_model, mut handle) = start_render_loop(element);

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(1));
                    lock.assert_created(MockNode(2));
                    lock.assert_created(MockNode(3));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn nested_suspense() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (sender, receiver) = async_channel::bounded::<()>(1);

                let inner = Suspense::builder()
                    .fallback(node(8))
                    .children(vec![delayed(&receiver, 2)])
                    .build()
                    .into();
                let element = Suspense::builder()
                    .fallback(node(9))
                    .children(vec![node(1), inner, node(3)])
                    .build()
                    .into();
                let (inner_object_model, mut handle) = start_render_loop(element);

                // The inner boundary catches the suspending component, so the outer one is shown
                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(1));
                    lock.assert_created(MockNode(8));
                    lock.assert_created(MockNode(3));
                    lock.assert_noop();
                }

                sender.send(()).await.unwrap();
                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(2));
                    lock.assert_removed(MockNode(8));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn suspense_defers_effects() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                static EFFECT_RUNS: AtomicUsize = AtomicUsize::new(0);
                let (sender, receiver) = async_channel::bounded::<()>(1);

                #[derive(PartialEq)]
                struct WithEffect;

                #[async_trait]
                impl Component for WithEffect {
                    type Error = RecvError;
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        use_effect((), |_| {
                            EFFECT_RUNS.fetch_add(1, Ordering::SeqCst);
                        });
                        Ok(Element::Node(MockNode(1), Vec::new()))
                    }
                }

                let element = Suspense::builder()
                    .fallback(node(9))
                    .children(vec![
                        Element::Component(Arc::new(WithEffect)),
                        delayed(&receiver, 2),
                    ])
                    .build()
                    .into();
                let (inner_object_model, mut handle) = start_render_loop(element);

                next_render_cycle(&inner_object_model, &mut handle).await;
                assert_eq!(
                    EFFECT_RUNS.load(Ordering::SeqCst),
                    0,
                    "Effect ran while the boundary showed its fallback"
                );

                sender.send(()).await.unwrap();
                next_render_cycle(&inner_object_model, &mut handle).await;
                assert_eq!(EFFECT_RUNS.load(Ordering::SeqCst), 1);

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn update_and_remove_suspended_boundary() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (_sender, receiver) = async_channel::bounded::<()>(1);

                struct Toggle(Receiver<()>);

                impl PartialEq for Toggle {
                    fn eq(&self, _: &Self) -> bool {
                        true
                    }
                }

                #[async_trait]
                impl Component for Toggle {
                    type Error = RecvError;
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let step = use_state(|| 0i32);

                        if *step < 2 {
                            let step = step.clone();
                            tokio::spawn(async move { step.update(|step| *step + 1) });
                        }

                        if *step == 2 {
                            return Ok(node(5));
                        }

                        Ok(Suspense::builder()
                            .fallback(node(9 + *step))
                            .children(vec![delayed(&self.0, 2)])
                            .build()
                            .into())
                    }
                }

                let (inner_object_model, mut handle) =
                    start_render_loop(Element::Component(Arc::new(Toggle(receiver))));

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(9));
                    lock.assert_noop();
                }

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_updated(MockNode(10));
                    lock.assert_noop();
                }

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(5));
                    lock.assert_removed(MockNode(10));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn reorder_keyed_children_of_suspended_boundary() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let (sender, receiver) = async_channel::bounded::<()>(1);

                #[derive(PartialEq)]
                struct Reorder;

                #[async_trait]
                impl Component for Reorder {
                    type Error = RecvError;
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        let step = use_state(|| 0i32);
                        if *step == 0 {
                            let step = step.clone();
                            tokio::spawn(async move { step.update(|step| *step + 1) });
                        }
                        let ids = if *step == 0 { [1, 2] } else { [2, 1] };
                        Ok(Element::Fragment(
                            ids.into_iter().map(|id| keyed(id, node(id))).collect(),
                        ))
                    }
                }

                let element = Suspense::builder()
                    .fallback(node(9))
                    .children(vec![
                        Element::Component(Arc::new(Reorder)),
                        delayed(&receiver, 3),
                    ])
                    .build()
                    .into();
                let (inner_object_model, mut handle) = start_render_loop(element);

                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(9));
                    lock.assert_noop();
                }

                // The detached children are reordered without touching the object model
                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let lock = inner_object_model.lock().unwrap();
                    lock.assert_noop();
                    assert!(lock.moved.is_empty(), "Detached nodes were moved");
                }

                sender.send(()).await.unwrap();
                next_render_cycle(&inner_object_model, &mut handle).await;
                {
                    let mut lock = inner_object_model.lock().unwrap();
                    lock.assert_created(MockNode(2));
                    lock.assert_created(MockNode(1));
                    lock.assert_created(MockNode(3));
                    lock.assert_removed(MockNode(9));
                    lock.assert_noop();
                }

                handle.abort();
            })
            .await;
    }
}
