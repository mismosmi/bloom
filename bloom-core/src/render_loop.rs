use std::{any::Any, sync::Arc};

use async_channel::{bounded, unbounded, Receiver, Sender};
use futures_util::{
    future::RemoteHandle,
    task::{Spawn, SpawnExt},
};

use crate::{
    component::{AnyComponent, ComponentDiff},
    hook::Hook,
    render_queue::{RenderQueue, RenderQueueItem},
    state::StateUpdate,
    suspense::{run_or_suspend, RunOrSuspendResult},
    Element,
};

pub(crate) struct TreeComponent<N, E> {
    component: Arc<dyn AnyComponent<Node = N, Error = E> + Sync>,
    state: Vec<Arc<dyn Any + Send + Sync>>,
    updates: Receiver<StateUpdate>,
    updater: Sender<StateUpdate>,
    render_result: Option<RemoteHandle<Result<Element<N, E>, E>>>,
    child: Option<Box<TreeNode<N, E>>>,
}

impl<N, E> TreeComponent<N, E> {
    fn new(component: Arc<dyn AnyComponent<Node = N, Error = E> + Sync>) -> Self {
        let (update_sender, update_receiver) = unbounded::<StateUpdate>();
        Self {
            component,
            state: Vec::new(),
            updates: update_receiver,
            updater: update_sender,
            child: None,
            render_result: None,
        }
    }
}

pub(crate) enum TreeNode<N, E> {
    Component(TreeComponent<N, E>),
    Node(Arc<N>, Vec<TreeNode<N, E>>),
    Fragment(Vec<TreeNode<N, E>>),
}

impl<N, E> TreeNode<N, E> {
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
                return None;
            }
        }
    }
}

pub trait ObjectModel {
    type Node;
    fn start(&mut self) {
        // Do nothing by default
    }
    fn create(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    );
    fn remove(&mut self, node: &Arc<Self::Node>, parent: &Arc<Self::Node>);
    fn update(&mut self, node: &Arc<Self::Node>, next: &Arc<Self::Node>);
    fn finalize(&mut self) {
        // Do nothing by default
    }
}

pub async fn render_loop<N, E, S, P>(
    root: Arc<N>,
    element: Element<N, E>,
    spawner: S,
    mut object_model: P,
) -> Result<(), E>
where
    N: Send + 'static,
    E: Send + 'static,
    S: Spawn,
    P: ObjectModel<Node = N>,
{
    let mut render_queue = RenderQueue::new();

    let mut tree_root = TreeNode::from(element);

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    while let Ok(_) = signal_receiver.recv().await {
        render_queue.reload(&mut tree_root, Arc::clone(&root), None);

        while let Some(item) = render_queue.next() {
            match item {
                RenderQueueItem::Create {
                    current,
                    parent,
                    sibling,
                } => match unsafe { &mut *current } {
                    TreeNode::Component(component) => render_component(
                        component,
                        &mut render_queue,
                        &signal_sender,
                        &parent,
                        &sibling,
                        &spawner,
                    )?,
                    TreeNode::Node(node, children) => {
                        object_model.create(node, &parent, &sibling);

                        let mut sibling = None;
                        for child in children.iter_mut().rev() {
                            render_queue.create(child, Arc::clone(node), sibling);
                            sibling = child.get_first_node();
                        }
                    }
                    TreeNode::Fragment(children) => {
                        let mut sibling = sibling;
                        for child in children.iter_mut().rev() {
                            render_queue.create(child, Arc::clone(&parent), sibling);
                            sibling = child.get_first_node();
                        }
                    }
                },
                RenderQueueItem::Reload {
                    current,
                    parent,
                    sibling,
                } => match unsafe { &mut *current } {
                    TreeNode::Component(component) => match component.child {
                        Some(ref mut child) => {
                            if component.updates.is_empty() {
                                render_queue.reload(child.as_mut(), parent, sibling)
                            } else {
                                render_component(
                                    component,
                                    &mut render_queue,
                                    &signal_sender,
                                    &parent,
                                    &sibling,
                                    &spawner,
                                )?
                            }
                        }
                        None => render_component(
                            component,
                            &mut render_queue,
                            &signal_sender,
                            &parent,
                            &sibling,
                            &spawner,
                        )?,
                    },
                    TreeNode::Node(node, children) => {
                        let mut sibling = None;
                        for child in children.iter_mut().rev() {
                            render_queue.reload(child, node.clone(), sibling);
                            sibling = child.get_first_node();
                        }
                    }
                    TreeNode::Fragment(children) => {
                        let mut sibling = sibling;
                        for child in children.iter_mut().rev() {
                            render_queue.reload(child, parent.clone(), sibling);
                            sibling = child.get_first_node();
                        }
                    }
                },
                RenderQueueItem::Update {
                    current,
                    next,
                    parent,
                    sibling,
                } => {
                    let current_node = unsafe { &mut *current };
                    match (current_node, next) {
                        (
                            TreeNode::Component(ref mut current_component),
                            Element::Component(next_component),
                        ) => match next_component.compare(current_component.component.as_any()) {
                            ComponentDiff::Equal => {
                                render_queue.reload(unsafe { &mut *current }, parent, sibling)
                            }
                            ComponentDiff::NewProps => {
                                current_component.component = next_component;
                                render_component(
                                    current_component,
                                    &mut render_queue,
                                    &signal_sender,
                                    &parent,
                                    &sibling,
                                    &spawner,
                                )?;
                            }
                            ComponentDiff::NewType => {
                                replace_node(
                                    unsafe { &mut *current },
                                    Element::Component(next_component),
                                    &mut render_queue,
                                    parent,
                                    sibling,
                                );
                            }
                        },
                        (
                            TreeNode::Node(current, current_children),
                            Element::Node(next, next_children),
                        ) => {
                            let next = Arc::new(next);
                            object_model.update(current, &next);
                            *current = Arc::clone(&next);
                            update_children(
                                current_children,
                                next_children,
                                &mut render_queue,
                                &next,
                                &None,
                            );
                        }
                        (
                            TreeNode::Fragment(current_children),
                            Element::Fragment(next_children),
                        ) => update_children(
                            current_children,
                            next_children,
                            &mut render_queue,
                            &parent,
                            &sibling,
                        ),
                        (current_node, next) => {
                            replace_node(current_node, next, &mut render_queue, parent, sibling)
                        }
                    }
                }
                RenderQueueItem::Remove { current, parent } => match current {
                    TreeNode::Component(component) => {
                        if let Some(child) = component.child {
                            render_queue.remove(*child, parent);
                        }
                    }
                    TreeNode::Node(node, children) => {
                        object_model.remove(&node, &parent);
                        for child in children {
                            render_queue.remove(child, Arc::clone(&node));
                        }
                    }
                    TreeNode::Fragment(children) => {
                        for child in children {
                            render_queue.remove(child, Arc::clone(&parent));
                        }
                    }
                },
            }
        }
    }

    Ok(())
}

fn update_children<N, E>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    parent: &Arc<N>,
    sibling: &Option<Arc<N>>,
) {
    let range = 0..std::cmp::max(tree_nodes.len(), elements.len());
    let mut nodes_iter = tree_nodes
        .drain(..)
        .collect::<Vec<TreeNode<N, E>>>()
        .into_iter();
    tree_nodes.shrink_to(elements.len());
    let mut elements_iter = elements.into_iter();
    for _i in range {
        let tree_node = nodes_iter.next();
        let element = elements_iter.next();

        match (tree_node, element) {
            (None, None) => panic!("Iterator running beyond finish"),
            (Some(tree_node), None) => render_queue.remove(tree_node, parent.clone()),
            (None, Some(element)) => {
                let mut tree_node = TreeNode::from(element);
                render_queue.create(&mut tree_node, Arc::clone(parent), sibling.clone());
                tree_nodes.push(tree_node);
            }
            (Some(mut tree_node), Some(element)) => {
                render_queue.update(&mut tree_node, element, parent.clone(), sibling.clone());
                tree_nodes.push(tree_node);
            }
        }
    }
}

fn render_component<N, E, S>(
    tree_component: &mut TreeComponent<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    signal_sender: &Sender<()>,
    parent: &Arc<N>,
    sibling: &Option<Arc<N>>,
    spawner: &S,
) -> Result<(), E>
where
    N: Send + 'static,
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
    );
    let result = run_or_suspend(async_context::provide_async_context(
        hook,
        component.render(),
    ));

    Ok(match result {
        RunOrSuspendResult::Done(element) => {
            tree_component.render_result = None;
            match tree_component.child {
                Some(ref mut node) => {
                    render_queue.update(node.as_mut(), element?, parent.clone(), sibling.clone())
                }
                None => {
                    let mut tree_node = TreeNode::from(element?);
                    render_queue.create(&mut tree_node, parent.clone(), sibling.clone());
                    tree_component.child = Some(Box::new(tree_node));
                }
            }
        }
        RunOrSuspendResult::Suspend(render_future) => {
            let signal_sender = signal_sender.clone();
            tree_component.render_result = Some(
                spawner
                    .spawn_with_handle(async move {
                        let result = render_future.await;
                        let _ = signal_sender.try_send(());
                        result
                    })
                    .expect("Failed to spawn async task"),
            );
        }
    })
}

fn replace_node<N, E>(
    node: &mut TreeNode<N, E>,
    element: Element<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    parent: Arc<N>,
    sibling: Option<Arc<N>>,
) {
    let mut old_node = TreeNode::from(element);
    std::mem::swap(node, &mut old_node);
    render_queue.create(node, Arc::clone(&parent), sibling);
    render_queue.remove(old_node, parent);
}
