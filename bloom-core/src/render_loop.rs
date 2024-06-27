use std::{any::Any, collections::HashMap, iter::repeat, sync::Arc};

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
    state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
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
            state: HashMap::new(),
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
    let mut tree_root = TreeNode::from(element);
    let mut render_queue = RenderQueue::new();

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    while let Ok(_) = signal_receiver.recv().await {
        render_queue.reload(&mut tree_root, root.clone(), None);

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
                        for child in children.iter_mut().rev() {
                            render_queue.create(child, Arc::clone(node), None);
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
                            *current = next.clone();
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
    mut elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    parent: &Arc<N>,
    sibling: &Option<Arc<N>>,
) {
    let old_len = tree_nodes.len();

    for tree_node in tree_nodes.drain(elements.len()..).rev() {
        render_queue.remove(tree_node, parent.clone());
    }

    tree_nodes.shrink_to_fit();

    for element in elements.drain(tree_nodes.len()..) {
        tree_nodes.push(TreeNode::from(element));
    }

    for tree_node in tree_nodes.iter_mut().skip(old_len).rev() {
        render_queue.create(tree_node, parent.clone(), sibling.clone());
    }

    let mut sibling = sibling.clone();
    for (tree_node, element) in tree_nodes.iter_mut().zip(elements.into_iter()).rev() {
        let next_sibling = tree_node.get_first_node();
        render_queue.update(tree_node, element, parent.clone(), sibling.clone());
        sibling = next_sibling;
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
                    let tree_node = TreeNode::from(element?);
                    let mut child = Box::new(tree_node);
                    render_queue.create(child.as_mut(), parent.clone(), sibling.clone());
                    tree_component.child = Some(child);
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

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        task::{Context, Poll},
    };

    use async_trait::async_trait;
    use futures_util::{
        stream::FuturesUnordered,
        task::{noop_waker, Spawn},
        Future, FutureExt,
    };

    use crate::{use_state, Component, Element, ObjectModel};

    struct MockObjectModel {
        to_create: Vec<MockNode>,
        to_update: Vec<MockNode>,
        to_remove: Vec<MockNode>,
    }

    #[derive(Debug, PartialEq)]
    struct MockNode(i32);

    impl ObjectModel for MockObjectModel {
        type Node = MockNode;
        fn create(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            _parent: &std::sync::Arc<Self::Node>,
            _sibling: &Option<std::sync::Arc<Self::Node>>,
        ) {
            println!("create({:?})", &node);
            assert_eq!(
                node.as_ref(),
                &self.to_create.pop().expect("Too many create-calls"),
                "Created node mismatch"
            );
        }

        fn update(&mut self, node: &std::sync::Arc<Self::Node>, next: &std::sync::Arc<Self::Node>) {
            println!("update({:?}, {:?})", &node, &next);
            assert_eq!(
                next.as_ref(),
                &self.to_update.pop().expect("Too many update-calls"),
                "Updated node mismatch"
            );
        }

        fn remove(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            _parent: &std::sync::Arc<Self::Node>,
        ) {
            println!("remove({:?})", &node);
            assert_eq!(
                node.as_ref(),
                &self.to_remove.pop().expect("Too many update-calls"),
                "Removed node mismatch"
            );
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

    #[test]
    fn render_basic_component() {
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

        let root = Arc::new(MockNode(0));
        let element = Element::Component(Arc::new(MockComponent));
        let object_model = MockObjectModel {
            to_create: vec![MockNode(0)],
            to_update: Vec::new(),
            to_remove: Vec::new(),
        };
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut task = Box::pin(super::render_loop(
            root,
            element,
            TokioSpawner,
            object_model,
        ));

        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
    }

    #[test]
    fn with_callback() {
        #[derive(PartialEq)]
        struct AutoCounter;

        #[async_trait]
        impl Component for AutoCounter {
            type Error = ();
            type Node = MockNode;
            async fn render(
                self: Arc<Self>,
            ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                let counter = use_state::<i32>();
                if *counter == 0 {
                    counter.update(|count| *count + 1);
                }
                Ok(Element::Node(MockNode(*counter), Vec::new()))
            }
        }
        let root = Arc::new(MockNode(0));
        let element = Element::Component(Arc::new(AutoCounter));
        let object_model = MockObjectModel {
            to_create: vec![MockNode(0)],
            to_update: vec![MockNode(1)],
            to_remove: Vec::new(),
        };
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut task = Box::pin(super::render_loop(
            root,
            element,
            TokioSpawner,
            object_model,
        ));

        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
    }

    #[test]
    fn update_order() {
        #[derive(PartialEq)]
        struct MultiContent;

        #[async_trait]
        impl Component for MultiContent {
            type Error = ();
            type Node = MockNode;
            async fn render(
                self: Arc<Self>,
            ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                let counter = use_state::<i32>();
                if *counter == 0 {
                    counter.update(|count| *count + 1);
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
        let root = Arc::new(MockNode(0));
        let element = Element::Component(Arc::new(MultiContent));
        let object_model = MockObjectModel {
            to_create: vec![MockNode(5), MockNode(4), MockNode(3), MockNode(0)],
            to_update: vec![MockNode(5), MockNode(4), MockNode(3), MockNode(1)],
            to_remove: Vec::new(),
        };
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut task = Box::pin(super::render_loop(
            root,
            element,
            TokioSpawner,
            object_model,
        ));

        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
        assert_eq!(task.poll_unpin(&mut cx), Poll::Pending);
    }
}
