use std::{
    any::Any,
    collections::HashMap,
    mem,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use async_channel::{bounded, unbounded, Receiver, Sender};
use futures_util::{
    future,
    task::{Spawn, SpawnExt},
    Future,
};

use crate::{
    commit::{Changes, Cleanups, ComponentId},
    component::{AnyComponent, ComponentDiff},
    context::ContextMap,
    effect::Effect,
    hook::Hook,
    render_context::RenderContext,
    state::StateUpdate,
    suspense::{run_or_suspend, RunOrSuspendResult},
    Element, Key,
};

type RenderOutput<N, E> = (Result<Element<N, E>, E>, Hook);
type RenderFuture<N, E> = Pin<Box<dyn Future<Output = RenderOutput<N, E>> + Send>>;
type Visit<'a, N, E> = Pin<Box<dyn Future<Output = Result<Changes<N>, E>> + Send + 'a>>;

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(0);

/// Levels of nested nodes that are visited in one task before continuing in a new one
const TASK_DEPTH: usize = 64;

/// What a render pass does with a subtree
enum Work<N, E>
where
    N: From<String>,
{
    Create,
    Reload,
    Update(Element<N, E>),
}

/// What a render pass does with a component
enum ComponentWork<N, E> {
    Create,
    Reload,
    NewProps(Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>),
}

/// Shared by all tasks of the render loop
struct Pass<S> {
    spawner: Arc<S>,
    signal: Sender<()>,
}

impl<S> Clone for Pass<S> {
    fn clone(&self) -> Self {
        Self {
            spawner: Arc::clone(&self.spawner),
            signal: self.signal.clone(),
        }
    }
}

pub(crate) struct TreeComponent<N, E>
where
    N: From<String>,
{
    id: ComponentId,
    component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
    updates: Receiver<StateUpdate>,
    updater: Sender<StateUpdate>,
    /// A render that suspended in an earlier render pass
    render_result: Option<RenderFuture<N, E>>,
    child: Option<Box<TreeNode<N, E>>>,
    refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
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
            id: NEXT_COMPONENT_ID.fetch_add(1, Ordering::Relaxed),
            component,
            state: HashMap::new(),
            updates: update_receiver,
            updater: update_sender,
            child: None,
            render_result: None,
            refs: HashMap::new(),
            deferred_effects: None,
        }
    }

    /// Whether the component has to render in this render pass
    fn needs_render(&mut self, work: ComponentWork<N, E>) -> bool {
        match work {
            ComponentWork::Create => true,
            ComponentWork::NewProps(component) => {
                self.component = component;
                true
            }
            // A suspended render that is still waiting is replaced if the state changed
            ComponentWork::Reload => {
                !self.updates.is_empty() || (self.render_result.is_none() && self.child.is_none())
            }
        }
    }

    /// Picks up a suspended render that has finished and reloads the child
    async fn reload<S>(&mut self, ctx: RenderContext<N>, pass: &Pass<S>) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        if let Some(render_result) = self.render_result.take() {
            match run_or_suspend(render_result) {
                RunOrSuspendResult::Suspend(render_result) => {
                    self.render_result = Some(render_result)
                }
                RunOrSuspendResult::Done(output) => {
                    return self.apply_render(output, ctx, pass).await
                }
            }
        }
        match self.child.as_mut() {
            Some(child) => visit(child, Work::Reload, ctx, pass).await,
            None => Ok(Changes::default()),
        }
    }

    /// Renders the component with its pending state updates applied.
    /// A render that does not finish on its first poll suspends and keeps running on the spawner,
    /// its result is picked up by a later render pass.
    async fn render<S>(&mut self, ctx: RenderContext<N>, pass: &Pass<S>) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        while let Ok(state_update) = self.updates.try_recv() {
            state_update.apply(&mut self.state);
        }

        // A newer render replaces one that suspended before
        self.render_result = None;
        let hook = Hook::new(
            pass.signal.clone(),
            self.updater.clone(),
            self.state.clone(),
            self.refs.clone(),
            ctx.context.clone(),
        );
        let component = Arc::clone(&self.component);
        match run_or_suspend(Box::pin(async_context::provide_async_context(
            hook,
            component.render(),
        ))) {
            RunOrSuspendResult::Done(output) => self.apply_render(output, ctx, pass).await,
            RunOrSuspendResult::Suspend(render_future) => {
                let signal = pass.signal.clone();
                self.render_result = Some(Box::pin(
                    pass.spawner
                        .spawn_with_handle(async move {
                            let output = render_future.await;
                            let _ = signal.try_send(());
                            output
                        })
                        .expect("Failed to spawn async task"),
                ));
                Ok(Changes::default())
            }
        }
    }

    /// Queues the effects of a finished render and diffs its output
    async fn apply_render<S>(
        &mut self,
        (element, hook): RenderOutput<N, E>,
        ctx: RenderContext<N>,
        pass: &Pass<S>,
    ) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        let mut changes = Changes::default();
        // Effects of a detached component only run once its nodes are shown
        if ctx.detached {
            self.deferred_effects = Some(hook.effects);
        } else {
            changes.queue_effects(self.id, hook.effects);
        }
        self.state = hook.state;
        self.refs = hook.refs;
        let element = element?;
        match self.child.as_mut() {
            Some(child) => changes.append(visit(child, Work::Update(element), ctx, pass).await?),
            None => {
                let mut child = Box::new(TreeNode::from(element));
                changes.append(visit(&mut child, Work::Create, ctx, pass).await?);
                self.child = Some(child);
            }
        }
        Ok(changes)
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
    Component(Box<TreeComponent<N, E>>),
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
    /// Converted with a stack instead of recursion, so deeply nested elements fit on the stack
    fn from(element: Element<N, E>) -> Self {
        let mut stack: Vec<PendingTreeNode<N, E>> = Vec::new();
        let mut element = element;
        loop {
            let mut finished = match element {
                Element::Component(component) => {
                    Some(TreeNode::Component(Box::new(TreeComponent::new(component))))
                }
                Element::Node(node, children) => {
                    stack.push(PendingTreeNode::new(PendingKind::Node(node), children));
                    None
                }
                Element::Fragment(children) => {
                    stack.push(PendingTreeNode::new(PendingKind::Fragment, children));
                    None
                }
                Element::Provider(value, children) => {
                    stack.push(PendingTreeNode::new(PendingKind::Provider(value), children));
                    None
                }
                Element::Keyed(key, element) => {
                    stack.push(PendingTreeNode::new(
                        PendingKind::Keyed(key),
                        vec![*element],
                    ));
                    None
                }
                Element::Suspense(fallback, children) => {
                    stack.push(PendingTreeNode::new(
                        PendingKind::Suspense(*fallback),
                        children,
                    ));
                    None
                }
            };
            // Finishes nodes until one of them has a child left to convert
            element = loop {
                let Some(pending) = stack.last_mut() else {
                    return finished.expect("Converted an element without a result");
                };
                if let Some(node) = finished.take() {
                    pending.children.push(node);
                }
                if let Some(child) = pending.elements.next() {
                    break child;
                }
                finished = stack.pop().map(PendingTreeNode::finish);
            };
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
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            match node {
                Self::Component(component) => {
                    if component.render_result.is_some() {
                        return true;
                    }
                    stack.extend(component.child.as_deref());
                }
                Self::Node(_, children)
                | Self::Fragment(children)
                | Self::Provider(_, children) => stack.extend(children),
                Self::Keyed(_, child) => stack.push(child),
                Self::Suspense(suspense) => {
                    if let SuspenseState::Fallback(fallback) = &suspense.state {
                        stack.push(fallback);
                    }
                }
            }
        }
        false
    }

    /// Passes a detached subtree to the object model, inserting it before the sibling.
    /// Effects that were held back while it was detached are queued.
    fn mount(&mut self, parent: &Arc<N>, sibling: &Option<Arc<N>>, changes: &mut Changes<N>) {
        // Children are pushed in reverse so they are mounted in document order
        let mut stack = vec![(self, Arc::clone(parent), sibling.clone())];
        while let Some((node, parent, sibling)) = stack.pop() {
            match node {
                Self::Component(component) => {
                    if let Some(effects) = component.deferred_effects.take() {
                        changes.queue_effects(component.id, effects);
                    }
                    if let Some(child) = component.child.as_deref_mut() {
                        stack.push((child, parent, sibling));
                    }
                }
                Self::Node(node, children) => {
                    changes.create_node(node, &parent, &sibling);
                    for child in children.iter_mut().rev() {
                        stack.push((child, Arc::clone(node), None));
                    }
                }
                Self::Fragment(children) | Self::Provider(_, children) => {
                    for child in children.iter_mut().rev() {
                        stack.push((child, Arc::clone(&parent), sibling.clone()));
                    }
                }
                Self::Keyed(_, child) => stack.push((child, parent, sibling)),
                Self::Suspense(suspense) => match &mut suspense.state {
                    SuspenseState::Unsettled(_) => {}
                    SuspenseState::Fallback(fallback) => stack.push((fallback, parent, sibling)),
                    SuspenseState::Revealed => {
                        for child in suspense.children.iter_mut().rev() {
                            stack.push((child, Arc::clone(&parent), sibling.clone()));
                        }
                    }
                },
            }
        }
    }

    fn key(&self) -> Option<Key> {
        match self {
            Self::Keyed(key, _) => Some(*key),
            _ => None,
        }
    }
}

/// A node whose children are still being converted from elements
struct PendingTreeNode<N, E>
where
    N: From<String>,
{
    kind: PendingKind<N, E>,
    elements: std::vec::IntoIter<Element<N, E>>,
    children: Vec<TreeNode<N, E>>,
}

enum PendingKind<N, E>
where
    N: From<String>,
{
    Node(N),
    Fragment,
    Provider(Arc<dyn Any + Send + Sync>),
    Keyed(Key),
    Suspense(Element<N, E>),
}

impl<N, E> PendingTreeNode<N, E>
where
    N: From<String>,
{
    fn new(kind: PendingKind<N, E>, elements: Vec<Element<N, E>>) -> Self {
        Self {
            kind,
            children: Vec::with_capacity(elements.len()),
            elements: elements.into_iter(),
        }
    }

    fn finish(mut self) -> TreeNode<N, E> {
        match self.kind {
            PendingKind::Node(node) => TreeNode::Node(Arc::new(node), self.children),
            PendingKind::Fragment => TreeNode::Fragment(self.children),
            PendingKind::Provider(value) => TreeNode::Provider(value, self.children),
            PendingKind::Keyed(key) => TreeNode::Keyed(
                key,
                Box::new(self.children.pop().expect("Keyed element without child")),
            ),
            PendingKind::Suspense(fallback) => TreeNode::Suspense(TreeSuspense {
                state: SuspenseState::Unsettled(fallback),
                children: self.children,
            }),
        }
    }
}

/// The root of the tree, dropped with a stack instead of recursion
struct TreeRoot<N, E>(TreeNode<N, E>)
where
    N: From<String>;

impl<N, E> Drop for TreeRoot<N, E>
where
    N: From<String>,
{
    fn drop(&mut self) {
        let root = mem::replace(&mut self.0, TreeNode::Fragment(Vec::new()));
        remove(root, None, &mut Changes::default());
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
/// Every component that renders is rendered and diffed in its own task on the spawner, so with a
/// multi-threaded spawner sibling components render in parallel.
/// The spawner is shared between these tasks, so it has to be `Send` and `Sync`.
///
/// Each render pass has two phases:
/// 1. Rendering visits the tree and collects the changes to the object model in document order,
///    the host environment is not touched.
///    A component that does not finish its render on the first poll suspends and its
///    result is picked up by a later render pass.
/// 2. The commit runs synchronously in one go after `ObjectModel::start`:
///    first the cleanups of removed components and of effects that run again,
///    then the queued changes to the object model, and finally the effects.
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
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
    P: ObjectModel<Node = N>,
{
    let mut tree_root = TreeRoot(TreeNode::from(element));

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    object_model.subscribe(signal_sender.clone());

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    let pass = Pass {
        spawner: Arc::new(spawner),
        signal: signal_sender,
    };
    let mut cleanups = Cleanups::default();
    let mut created = false;

    while signal_receiver.recv().await.is_ok() {
        let ctx = RenderContext::new(root.clone(), None, object_model.get_context());
        // The first pass creates the whole tree, later passes only reload what changed
        let work = if created { Work::Reload } else { Work::Create };
        created = true;
        let changes = visit(&mut tree_root.0, work, ctx, &pass).await?;

        object_model.start().await;
        cleanups.commit(changes, &mut object_model);
        object_model.finalize().await;
    }

    Ok(())
}

/// Diffs a subtree and collects its changes in document order.
/// Components that render are visited in their own task on the spawner, so siblings are rendered
/// and diffed in parallel. Everything else is visited in place and continues in a new task
/// every `TASK_DEPTH` levels, so deeply nested trees fit on the stack.
fn visit<'a, N, E, S>(
    node: &'a mut TreeNode<N, E>,
    work: Work<N, E>,
    ctx: RenderContext<N>,
    pass: &'a Pass<S>,
) -> Visit<'a, N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    Box::pin(async move {
        let mut ctx = ctx;
        // Deeply nested nodes continue in a new task so they fit on the stack
        if ctx.depth >= TASK_DEPTH {
            return visit_in_task(node, work, ctx, pass).await;
        }
        ctx.depth += 1;
        match work {
            Work::Create => create(node, ctx, pass).await,
            Work::Reload => reload(node, ctx, pass).await,
            Work::Update(next) => update(node, next, ctx, pass).await,
        }
    })
}

/// Moves a subtree into a task on the spawner and puts it back once it is visited
async fn visit_in_task<N, E, S>(
    node: &mut TreeNode<N, E>,
    work: Work<N, E>,
    mut ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let mut subtree = mem::replace(node, TreeNode::Fragment(Vec::new()));
    let task_pass = pass.clone();
    ctx.depth = 0;
    let task = pass
        .spawner
        .spawn_with_handle(async move {
            let result = visit(&mut subtree, work, ctx, &task_pass).await;
            (subtree, result)
        })
        .expect("Failed to spawn async task");
    let (subtree, result) = task.await;
    *node = subtree;
    result
}

/// Visits the subtrees concurrently and joins their changes in order
async fn visit_all<N, E>(visits: Vec<Visit<'_, N, E>>) -> Result<Changes<N>, E> {
    let mut changes = Changes::default();
    for child_changes in future::try_join_all(visits).await? {
        changes.append(child_changes);
    }
    Ok(changes)
}

/// A component that renders is moved into a task on the spawner, so sibling components render
/// in parallel, and put back once it is visited. Other components are visited in place.
async fn visit_component<N, E, S>(
    node: &mut TreeNode<N, E>,
    work: ComponentWork<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let TreeNode::Component(component) = node else {
        unreachable!("Only components are visited as components")
    };
    if !component.needs_render(work) {
        return component.reload(ctx, pass).await;
    }

    let TreeNode::Component(mut component) = mem::replace(node, TreeNode::Fragment(Vec::new()))
    else {
        unreachable!("Only components are visited as components")
    };
    let task_pass = pass.clone();
    let mut ctx = ctx;
    ctx.depth = 0;
    let task = pass
        .spawner
        .spawn_with_handle(async move {
            let result = component.render(ctx, &task_pass).await;
            (component, result)
        })
        .expect("Failed to spawn async task");
    let (component, result) = task.await;
    *node = TreeNode::Component(component);
    result
}

async fn create<N, E, S>(
    node: &mut TreeNode<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let mut changes = Changes::default();
    match node {
        TreeNode::Component(_) => {
            return visit_component(node, ComponentWork::Create, ctx, pass).await;
        }
        TreeNode::Node(node, children) => {
            if !ctx.detached {
                changes.create_node(node, &ctx.parent, &ctx.sibling);
            }
            let children_ctx = ctx.with_parent(node.clone());
            changes.append(
                visit_all(
                    children
                        .iter_mut()
                        .map(|child| visit(child, Work::Create, children_ctx.clone(), pass))
                        .collect(),
                )
                .await?,
            );
        }
        // Children are created in order, so each one is inserted before the
        // fragment's sibling, which already exists, rather than before the next child
        TreeNode::Fragment(children) => changes.append(
            visit_all(
                children
                    .iter_mut()
                    .map(|child| visit(child, Work::Create, ctx.clone(), pass))
                    .collect(),
            )
            .await?,
        ),
        TreeNode::Provider(value, children) => {
            let children_ctx = ctx.with_context(value.clone());
            changes.append(
                visit_all(
                    children
                        .iter_mut()
                        .map(|child| visit(child, Work::Create, children_ctx.clone(), pass))
                        .collect(),
                )
                .await?,
            );
        }
        TreeNode::Keyed(_, child) => return visit(child, Work::Create, ctx, pass).await,
        // The fallback is only created once it is clear that the children suspend
        TreeNode::Suspense(suspense) => {
            let children_ctx = ctx.detach();
            changes.append(
                visit_all(
                    suspense
                        .children
                        .iter_mut()
                        .map(|child| visit(child, Work::Create, children_ctx.clone(), pass))
                        .collect(),
                )
                .await?,
            );
            changes.append(settle(suspense, ctx, pass).await?);
        }
    }
    Ok(changes)
}

/// The sibling of each child is the first node of the child after it before this render pass
fn sibling_anchors<N, E>(children: &[TreeNode<N, E>], last: Option<Arc<N>>) -> Vec<Option<Arc<N>>>
where
    N: From<String>,
{
    let mut sibling = last;
    let mut anchors = Vec::with_capacity(children.len());
    for child in children.iter().rev() {
        anchors.push(sibling);
        sibling = child.get_first_node();
    }
    anchors.reverse();
    anchors
}

async fn reload<N, E, S>(
    node: &mut TreeNode<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    match node {
        TreeNode::Component(_) => visit_component(node, ComponentWork::Reload, ctx, pass).await,
        TreeNode::Node(node, children) => {
            let anchors = sibling_anchors(children, None);
            visit_all(
                children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        let child_ctx = ctx.with_parent_and_sibling(node.clone(), sibling);
                        visit(child, Work::Reload, child_ctx, pass)
                    })
                    .collect(),
            )
            .await
        }
        TreeNode::Fragment(children) => {
            let anchors = sibling_anchors(children, ctx.sibling.clone());
            visit_all(
                children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        visit(child, Work::Reload, ctx.with_sibling(sibling), pass)
                    })
                    .collect(),
            )
            .await
        }
        TreeNode::Provider(value, children) => {
            let anchors = sibling_anchors(children, ctx.sibling.clone());
            visit_all(
                children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        let child_ctx = ctx.with_sibling_and_context(sibling, value.clone());
                        visit(child, Work::Reload, child_ctx, pass)
                    })
                    .collect(),
            )
            .await
        }
        TreeNode::Keyed(_, child) => visit(child, Work::Reload, ctx, pass).await,
        TreeNode::Suspense(suspense) => {
            let children_ctx = suspense.children_ctx(&ctx);
            let anchors = sibling_anchors(&suspense.children, children_ctx.sibling.clone());
            let mut changes = visit_all(
                suspense
                    .children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        visit(
                            child,
                            Work::Reload,
                            children_ctx.with_sibling(sibling),
                            pass,
                        )
                    })
                    .collect(),
            )
            .await?;
            if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                changes.append(visit(fallback, Work::Reload, ctx.clone(), pass).await?);
            }
            changes.append(settle(suspense, ctx, pass).await?);
            Ok(changes)
        }
    }
}

async fn update<N, E, S>(
    node: &mut TreeNode<N, E>,
    next: Element<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    // A changed key means a different element, so it is replaced instead of
    // updated in place. Removing the old element runs its cleanups.
    if node.key() != element_key(&next) {
        return replace_node(node, next, ctx, pass).await;
    }
    if let (TreeNode::Component(current), Element::Component(next_component)) = (&*node, &next) {
        let work = match next_component.compare(current.component.as_any()) {
            ComponentDiff::Equal => ComponentWork::Reload,
            ComponentDiff::NewProps => ComponentWork::NewProps(Arc::clone(next_component)),
            // Removing the old component runs its cleanups
            ComponentDiff::NewType => return replace_node(node, next, ctx, pass).await,
        };
        return visit_component(node, work, ctx, pass).await;
    }

    let mut changes = Changes::default();
    match (node, next) {
        (TreeNode::Node(current, current_children), Element::Node(next, next_children)) => {
            let next = Arc::new(next);
            if !ctx.detached {
                changes.update_node(current, &next);
            }
            *current = next.clone();
            changes.append(
                update_children(current_children, next_children, ctx.with_parent(next), pass)
                    .await?,
            );
        }
        (TreeNode::Fragment(current_children), Element::Fragment(next_children)) => {
            changes.append(update_children(current_children, next_children, ctx, pass).await?)
        }
        (TreeNode::Provider(_, current_children), Element::Provider(next_value, next_children)) => {
            changes.append(
                update_children(
                    current_children,
                    next_children,
                    ctx.with_context(next_value),
                    pass,
                )
                .await?,
            )
        }
        // The keys are equal, they were compared above
        (TreeNode::Keyed(_, current_child), Element::Keyed(_, next_element)) => {
            return visit(current_child, Work::Update(*next_element), ctx, pass).await;
        }
        (TreeNode::Suspense(suspense), Element::Suspense(next_fallback, next_children)) => {
            let children_ctx = suspense.children_ctx(&ctx);
            changes.append(
                update_children(&mut suspense.children, next_children, children_ctx, pass).await?,
            );
            // Once revealed, the fallback is never shown again
            if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                changes.append(
                    visit(fallback, Work::Update(*next_fallback), ctx.clone(), pass).await?,
                );
            }
            changes.append(settle(suspense, ctx, pass).await?);
        }
        (node, next) => return replace_node(node, next, ctx, pass).await,
    }
    Ok(changes)
}

/// Runs after everything below a suspense boundary has been visited
async fn settle<N, E, S>(
    suspense: &mut TreeSuspense<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let pending = !suspense.is_revealed() && suspense.children.iter().any(TreeNode::is_pending);
    let mut changes = Changes::default();
    match mem::replace(&mut suspense.state, SuspenseState::Revealed) {
        SuspenseState::Revealed => {}
        SuspenseState::Unsettled(fallback) if pending => {
            let mut fallback = Box::new(TreeNode::from(fallback));
            changes = visit(&mut fallback, Work::Create, ctx, pass).await?;
            suspense.state = SuspenseState::Fallback(fallback);
        }
        SuspenseState::Fallback(fallback) if pending => {
            suspense.state = SuspenseState::Fallback(fallback);
        }
        previous => {
            // A boundary inside a detached subtree is mounted along with it
            if !ctx.detached {
                for child in &mut suspense.children {
                    child.mount(&ctx.parent, &ctx.sibling, &mut changes);
                }
            }
            if let SuspenseState::Fallback(fallback) = previous {
                remove(*fallback, ctx.attached_parent(), &mut changes);
            }
        }
    }
    Ok(changes)
}

/// Removes a subtree that is no longer part of the tree.
/// The parent is none if the nodes were never passed to the object model.
fn remove<N, E>(node: TreeNode<N, E>, parent: Option<Arc<N>>, changes: &mut Changes<N>)
where
    N: From<String>,
{
    // The subtree is owned, so it is walked with a stack instead of recursion
    let mut stack = vec![(node, parent)];
    while let Some((node, parent)) = stack.pop() {
        match node {
            TreeNode::Component(component) => {
                changes.unmount(component.id);
                if let Some(child) = component.child {
                    stack.push((*child, parent));
                }
            }
            TreeNode::Node(node, children) => {
                if let Some(parent) = &parent {
                    changes.remove_node(&node, parent);
                }
                for child in children {
                    stack.push((child, parent.as_ref().map(|_| Arc::clone(&node))));
                }
            }
            TreeNode::Fragment(children) | TreeNode::Provider(_, children) => {
                for child in children {
                    stack.push((child, parent.clone()));
                }
            }
            TreeNode::Keyed(_, child) => stack.push((*child, parent)),
            TreeNode::Suspense(suspense) => {
                let children_parent = if suspense.is_revealed() {
                    parent.clone()
                } else {
                    None
                };
                if let SuspenseState::Fallback(fallback) = suspense.state {
                    stack.push((*fallback, parent));
                }
                for child in suspense.children {
                    stack.push((child, children_parent.clone()));
                }
            }
        }
    }
}

/// Children are matched by key if any of them has one and by position otherwise.
async fn update_children<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    if tree_nodes.iter().any(|node| node.key().is_some())
        || elements
            .iter()
            .any(|element| element_key(element).is_some())
    {
        update_keyed_children(tree_nodes, elements, ctx, pass).await
    } else {
        update_children_by_position(tree_nodes, elements, ctx, pass).await
    }
}

async fn update_children_by_position<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    mut elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let kept = elements.len().min(tree_nodes.len());
    let removed: Vec<TreeNode<N, E>> = tree_nodes.drain(kept..).collect();
    tree_nodes.shrink_to_fit();
    tree_nodes.extend(elements.split_off(kept).into_iter().map(TreeNode::from));

    // New children are appended before the sibling of the whole list
    let anchors = sibling_anchors(&tree_nodes[..kept], ctx.sibling.clone());
    let mut updates = elements.into_iter().zip(anchors);
    let mut changes = visit_all(
        tree_nodes
            .iter_mut()
            .map(|tree_node| match updates.next() {
                Some((element, sibling)) => visit(
                    tree_node,
                    Work::Update(element),
                    ctx.with_sibling(sibling),
                    pass,
                ),
                None => visit(tree_node, Work::Create, ctx.clone(), pass),
            })
            .collect(),
    )
    .await?;

    for tree_node in removed {
        remove(tree_node, ctx.attached_parent(), &mut changes);
    }
    Ok(changes)
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

async fn update_keyed_children<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
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

    // Matched children are moved first so the new children and replaced nodes can be
    // inserted relative to them later.
    // Going from the end, every child that is part of the longest increasing subsequence
    // of old indices is already in place, every other one is moved before the next matched child.
    let mut changes = Changes::default();
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
                changes.move_node(node, &ctx.parent, &anchor);
            }
        }
        if let Some(first_node) = tree_node.get_first_node() {
            anchor = Some(first_node);
        }
    }

    // New children are not created yet, so only matched children can be siblings.
    // Children are visited front to back, so new children that share a sibling
    // are still inserted in order.
    let mut anchors = Vec::with_capacity(tree_nodes.len());
    let mut sibling = ctx.sibling.clone();
    for (tree_node, matched) in tree_nodes.iter().zip(&matches).rev() {
        anchors.push(sibling.clone());
        if matched.is_some() {
            if let Some(first_node) = tree_node.get_first_node() {
                sibling = Some(first_node);
            }
        }
    }
    anchors.reverse();

    changes.append(
        visit_all(
            tree_nodes
                .iter_mut()
                .zip(matches)
                .zip(anchors)
                .map(|((tree_node, matched), sibling)| {
                    let work = match matched {
                        Some((_, element)) => Work::Update(element),
                        None => Work::Create,
                    };
                    visit(tree_node, work, ctx.with_sibling(sibling), pass)
                })
                .collect(),
        )
        .await?,
    );

    for old_node in old_nodes.into_iter().flatten() {
        remove(old_node, ctx.attached_parent(), &mut changes);
    }
    Ok(changes)
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

async fn replace_node<N, E, S>(
    node: &mut TreeNode<N, E>,
    element: Element<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Result<Changes<N>, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Send + Sync + 'static,
{
    let old_node = mem::replace(node, TreeNode::from(element));
    let mut changes = visit(node, Work::Create, ctx.clone(), pass).await?;
    remove(old_node, ctx.attached_parent(), &mut changes);
    Ok(changes)
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
        time::{Duration, Instant},
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

    #[tokio::test]
    async fn siblings_render_in_parallel() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                /// Blocks its render until its sibling has started rendering as well
                struct Rendezvous {
                    id: i32,
                    started: Arc<AtomicUsize>,
                }

                impl PartialEq for Rendezvous {
                    fn eq(&self, _: &Self) -> bool {
                        true
                    }
                }

                #[async_trait]
                impl Component for Rendezvous {
                    type Error = String;
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        self.started.fetch_add(1, Ordering::SeqCst);
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while self.started.load(Ordering::SeqCst) < 2 {
                            if Instant::now() > deadline {
                                return Err("Siblings rendered one after another".to_string());
                            }
                            std::thread::yield_now();
                        }
                        Ok(Element::Node(MockNode(self.id), Vec::new()))
                    }
                }

                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let root = Arc::new(MockNode(0));
                let loop_root = root.clone();
                let mut handle = tokio::task::spawn_local(async move {
                    let started = Arc::new(AtomicUsize::new(0));
                    let element = Element::fragment(
                        [1, 2]
                            .into_iter()
                            .map(|id| {
                                Element::Component(Arc::new(Rendezvous {
                                    id,
                                    started: started.clone(),
                                }))
                            })
                            .collect(),
                    );
                    let spawner = futures_executor::ThreadPool::builder()
                        .pool_size(2)
                        .create()
                        .unwrap();
                    super::render_loop(loop_root, element, spawner, object_model)
                        .await
                        .unwrap();
                });

                next_render_cycle(&inner_object_model, &mut handle).await;
                inner_object_model
                    .lock()
                    .unwrap()
                    .assert_children(&root, &[1, 2], &[1, 2], &[], 0);

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn commit_runs_cleanups_before_and_effects_after_node_operations() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                type Log = Arc<Mutex<Vec<String>>>;

                /// Effect argument that checks whether the child's node is shown
                #[derive(Clone)]
                struct Probe {
                    object_model: Arc<Mutex<InnerMockObjectModel>>,
                    root: Arc<MockNode>,
                    log: Log,
                }

                impl Hash for Probe {
                    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
                }

                impl Probe {
                    fn record(&self, event: &str) {
                        let shown = self
                            .object_model
                            .lock()
                            .unwrap()
                            .children_of(&self.root)
                            .contains(&1);
                        self.log
                            .lock()
                            .unwrap()
                            .push(format!("{} shown: {}", event, shown));
                    }
                }

                struct Child(Probe);

                impl PartialEq for Child {
                    fn eq(&self, _: &Self) -> bool {
                        true
                    }
                }

                #[async_trait]
                impl Component for Child {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        use_effect(self.0.clone(), |probe| {
                            probe.record("effect");
                            move || probe.record("cleanup")
                        });
                        Ok(Element::Node(MockNode(1), Vec::new()))
                    }
                }

                struct Parent(Probe);

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
                        let removed = use_state(|| false);
                        if *removed {
                            return Ok(().into());
                        }
                        tokio::spawn(async move { removed.update(|_| true) });
                        Ok(Element::Component(Arc::new(Child(self.0.clone()))))
                    }
                }

                let log = Log::default();
                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let root = Arc::new(MockNode(0));
                let probe = Probe {
                    object_model: inner_object_model.clone(),
                    root: root.clone(),
                    log: log.clone(),
                };
                let mut handle = tokio::task::spawn_local(async move {
                    let element = Element::Component(Arc::new(Parent(probe)));
                    super::render_loop(root, element, TokioSpawner, object_model)
                        .await
                        .unwrap();
                });

                for expected in [vec!["effect shown: true"], vec!["cleanup shown: true"]] {
                    next_render_cycle(&inner_object_model, &mut handle).await;
                    let entries: Vec<String> = log.lock().unwrap().drain(..).collect();
                    assert_eq!(entries, expected);
                }

                handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn deeply_nested_components() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                /// Renders a chain of nested components that ends in a node
                #[derive(PartialEq)]
                struct Nested(usize);

                #[async_trait]
                impl Component for Nested {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        Ok(match self.0 {
                            0 => Element::Node(MockNode(1), Vec::new()),
                            depth => Element::Component(Arc::new(Nested(depth - 1))),
                        })
                    }
                }

                // The second pass reloads the chain without rendering it again
                let mut harness = StepsHarness::start(vec![vec![], vec![]], |_| {
                    Element::Component(Arc::new(Nested(2000)))
                });
                let root = harness.root.clone();

                harness
                    .next_cycle()
                    .await
                    .assert_children(&root, &[1], &[1], &[], 0);
                harness.next_cycle().await.assert_noop();

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn spawns_only_for_rendering_components() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                #[derive(Clone)]
                struct CountingSpawner(Arc<AtomicUsize>);

                impl Spawn for CountingSpawner {
                    fn spawn_obj(
                        &self,
                        future: futures_util::task::FutureObj<'static, ()>,
                    ) -> Result<(), futures_util::task::SpawnError> {
                        self.0.fetch_add(1, Ordering::SeqCst);
                        tokio::spawn(future.map(|_| ()));
                        Ok(())
                    }
                }

                #[derive(PartialEq)]
                struct Leaf(i32);

                #[async_trait]
                impl Component for Leaf {
                    type Error = ();
                    type Node = MockNode;

                    async fn render(
                        self: Arc<Self>,
                    ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                        Ok(Element::Node(MockNode(self.0), Vec::new()))
                    }
                }

                // Only the last leaf gets new props in the second step
                let steps = vec![(1..=100).collect(), (1..100).chain([1000]).collect()];
                let render: fn(&[i32]) -> Element<MockNode, ()> = |ids| {
                    Element::fragment(
                        ids.iter()
                            .map(|id| Element::Component(Arc::new(Leaf(*id))))
                            .collect(),
                    )
                };

                let spawned = Arc::new(AtomicUsize::new(0));
                let inner_object_model = InnerMockObjectModel::new();
                let object_model = MockObjectModel(inner_object_model.clone());
                let spawner = CountingSpawner(spawned.clone());
                let mut handle = tokio::task::spawn_local(async move {
                    let root = Arc::new(MockNode(0));
                    let element = Element::Component(Arc::new(Steps { steps, render }));
                    super::render_loop(root, element, spawner, object_model)
                        .await
                        .unwrap();
                });

                // The next pass may already be rendering when a cycle is over,
                // so the spawns are counted once both passes are done
                next_render_cycle(&inner_object_model, &mut handle).await;
                next_render_cycle(&inner_object_model, &mut handle).await;
                // The first pass renders the parent and every leaf,
                // the second one the parent and the leaf with new props
                assert_eq!(spawned.load(Ordering::SeqCst), 101 + 2);

                handle.abort();
            })
            .await;
    }

    const DEPTH: i32 = 5_000;

    /// Nests DEPTH nodes, each node's id is its depth and the innermost node holds the value
    fn nested_nodes(value: i32) -> Element<MockNode, ()> {
        let mut element = Element::Node(MockNode(-value), Vec::new());
        for id in (1..=DEPTH).rev() {
            element = Element::Node(MockNode(id), vec![element]);
        }
        element
    }

    #[tokio::test]
    async fn deeply_nested_nodes() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                // The innermost node changes, then the whole tree is removed
                let mut harness =
                    StepsHarness::start(vec![vec![1], vec![2], vec![]], |step| match step {
                        [value] => nested_nodes(*value),
                        _ => Element::fragment(Vec::new()),
                    });
                let root = harness.root.clone();

                {
                    let mut lock = harness.next_cycle().await;
                    assert_eq!(lock.created.len(), DEPTH as usize + 1);
                    assert_eq!(lock.children_of(&root), [1]);
                    lock.created.clear();
                }
                {
                    let mut lock = harness.next_cycle().await;
                    assert_eq!(lock.updated.len(), DEPTH as usize + 1);
                    lock.updated.clear();
                    lock.assert_noop();
                }
                {
                    let mut lock = harness.next_cycle().await;
                    assert_eq!(lock.removed.len(), DEPTH as usize + 1);
                    lock.removed.clear();
                    assert_eq!(lock.children_of(&root), Vec::<i32>::new());
                    lock.assert_noop();
                }

                harness.handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn deeply_nested_nodes_in_suspense() {
        tokio::task::LocalSet::new()
            .run_until(async move {
                // The boundary is revealed right away and mounts the detached nodes
                let mut harness = StepsHarness::start(vec![vec![1]], |_| {
                    Suspense::<MockNode, ()>::builder()
                        .fallback(Element::Node(MockNode(0), Vec::new()))
                        .children(vec![nested_nodes(1)])
                        .build()
                        .into()
                });
                let root = harness.root.clone();

                {
                    let mut lock = harness.next_cycle().await;
                    assert_eq!(lock.created.len(), DEPTH as usize + 1);
                    lock.created.clear();
                    assert_eq!(lock.children_of(&root), [1]);
                    lock.assert_noop();
                }

                harness.handle.abort();
            })
            .await;
    }
}
