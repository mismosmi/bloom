use std::{
    any::Any,
    collections::HashMap,
    mem,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use async_channel::{unbounded, Receiver, Sender};

use crate::{
    commit::{Changes, ComponentId},
    component::AnyComponent,
    effect::Effect,
    render_context::RenderContext,
    state::StateUpdate,
    Element, Key,
};

use super::{visit::remove, RenderFuture};

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct TreeComponent<N, E>
where
    N: From<String>,
{
    pub(super) id: ComponentId,
    pub(super) component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    pub(super) state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
    pub(super) updates: Receiver<StateUpdate>,
    pub(super) updater: Sender<StateUpdate>,
    /// A render that suspended in an earlier render pass
    pub(super) render_result: Option<RenderFuture<N, E>>,
    pub(super) child: Option<Box<TreeNode<N, E>>>,
    pub(super) refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
    /// Effects of a detached component only run once its nodes are shown
    pub(super) deferred_effects: Option<Vec<(u64, Effect)>>,
}

impl<N, E> TreeComponent<N, E>
where
    N: From<String>,
{
    pub(super) fn new(component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>) -> Self {
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
}

pub(super) enum SuspenseState<N, E>
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

pub(super) struct TreeSuspense<N, E>
where
    N: From<String>,
{
    pub(super) state: SuspenseState<N, E>,
    pub(super) children: Vec<TreeNode<N, E>>,
}

impl<N, E> TreeSuspense<N, E>
where
    N: From<String>,
{
    pub(super) fn is_revealed(&self) -> bool {
        matches!(self.state, SuspenseState::Revealed)
    }

    /// Children are only passed to the object model once the boundary is revealed
    pub(super) fn children_ctx(&self, ctx: &RenderContext<N>) -> RenderContext<N> {
        if self.is_revealed() {
            ctx.clone()
        } else {
            ctx.detach()
        }
    }
}

pub(super) enum TreeNode<N, E>
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
    pub(super) fn from(element: Element<N, E>) -> Self {
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

    pub(super) fn get_first_node(&self) -> Option<Arc<N>> {
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
    pub(super) fn collect_nodes(&self, nodes: &mut Vec<Arc<N>>) {
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
    pub(super) fn is_pending(&self) -> bool {
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
    pub(super) fn mount(
        &mut self,
        parent: &Arc<N>,
        sibling: &Option<Arc<N>>,
        changes: &mut Changes<N>,
    ) {
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

    pub(super) fn key(&self) -> Option<Key> {
        match self {
            Self::Keyed(key, _) => Some(*key),
            _ => None,
        }
    }
}

/// A node whose children are still being converted from elements
pub(super) struct PendingTreeNode<N, E>
where
    N: From<String>,
{
    kind: PendingKind<N, E>,
    elements: std::vec::IntoIter<Element<N, E>>,
    children: Vec<TreeNode<N, E>>,
}

pub(super) enum PendingKind<N, E>
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
    pub(super) fn new(kind: PendingKind<N, E>, elements: Vec<Element<N, E>>) -> Self {
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
pub(super) struct TreeRoot<N, E>(pub(super) TreeNode<N, E>)
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

pub(super) fn element_key<N, E>(element: &Element<N, E>) -> Option<Key>
where
    N: From<String>,
{
    match element {
        Element::Keyed(key, _) => Some(*key),
        _ => None,
    }
}
