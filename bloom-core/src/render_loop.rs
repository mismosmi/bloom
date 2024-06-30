use std::{any::Any, collections::HashMap, pin::Pin, sync::Arc};

use async_channel::{bounded, unbounded, Receiver, Sender};
use futures_util::{
    future,
    task::{Spawn, SpawnExt},
    Future,
};

use crate::{
    component::{AnyComponent, ComponentDiff},
    hook::Hook,
    render_queue::{RenderContext, RenderQueue, RenderQueueItem},
    state::StateUpdate,
    suspense::{run_or_suspend, RunOrSuspendResult},
    Element,
};

pub(crate) struct TreeComponent<N, E> {
    component: Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
    updates: Receiver<StateUpdate>,
    updater: Sender<StateUpdate>,
    render_result: Option<Pin<Box<dyn Future<Output = (Result<Element<N, E>, E>, Hook)> + Send>>>,
    child: Option<Box<TreeNode<N, E>>>,
    refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
}

impl<N, E> TreeComponent<N, E> {
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
        }
    }
}

pub(crate) enum TreeNode<N, E> {
    Component(TreeComponent<N, E>),
    Node(Arc<N>, Vec<TreeNode<N, E>>),
    Fragment(Vec<TreeNode<N, E>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<TreeNode<N, E>>),
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
            Element::Provider(value, children) => {
                TreeNode::Provider(value, children.into_iter().map(TreeNode::from).collect())
            }
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
            Self::Provider(_, children) => {
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
    fn update(&mut self, node: &Arc<Self::Node>, next: &Arc<Self::Node>);
    fn finalize(&mut self) -> impl Future<Output = ()> + Send {
        // Do nothing by default
        future::ready(())
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

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    while let Ok(_) = signal_receiver.recv().await {
        println!("start render cycle");
        object_model.start().await;
        {
            let mut render_queue = RenderQueue::new();
            render_queue.reload(
                &mut tree_root,
                RenderContext::new(root.clone(), None, Arc::default()),
            );

            while let Some(item) = render_queue.next() {
                println!("rendering item");
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
                            object_model.create(node, &ctx.parent, &ctx.sibling);
                            for child in children.iter_mut().rev() {
                                render_queue.create(child, ctx.with_parent(node.clone()));
                            }
                        }
                        TreeNode::Fragment(children) => {
                            let mut sibling = ctx.sibling.clone();
                            for child in children.iter_mut().rev() {
                                render_queue.create(child, ctx.with_sibling(sibling));
                                sibling = child.get_first_node();
                            }
                        }
                        TreeNode::Provider(value, children) => {
                            let mut sibling = ctx.sibling.clone();
                            for child in children.iter_mut().rev() {
                                render_queue.create(
                                    child,
                                    ctx.with_sibling_and_context(sibling, value.clone()),
                                );
                                sibling = child.get_first_node();
                            }
                        }
                    },
                    RenderQueueItem::Reload { current, ctx } => match unsafe { &mut *current } {
                        TreeNode::Component(component) => {
                            dbg!("reload component");
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
                                            render_queue
                                                .queue_effects(&component.component, hook.effects);
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
                            dbg!("reload node");
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
                    },
                    RenderQueueItem::Update { current, next, ctx } => {
                        dbg!("update item");
                        let current_node = unsafe { &mut *current };
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
                                    render_queue.move_cleanups(
                                        &current_component.component,
                                        &next_component,
                                    );
                                    current_component.component = next_component;
                                    render_component(
                                        current_component,
                                        &mut render_queue,
                                        &signal_sender,
                                        ctx,
                                        &spawner,
                                    )?;
                                }
                                ComponentDiff::NewType => {
                                    render_queue.queue_cleanups(&current_component.component);
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
                                object_model.update(current, &next);
                                *current = next.clone();
                                update_children(
                                    current_children,
                                    next_children,
                                    &mut render_queue,
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
                                ctx,
                            ),
                            (
                                TreeNode::Provider(_, current_children),
                                Element::Provider(next_value, next_children),
                            ) => update_children(
                                current_children,
                                next_children,
                                &mut render_queue,
                                ctx.with_context(next_value),
                            ),
                            (current_node, next) => {
                                if let TreeNode::Component(current_component) = current_node {
                                    render_queue.queue_cleanups(&current_component.component);
                                }
                                replace_node(current_node, next, &mut render_queue, ctx)
                            }
                        }
                    }
                    RenderQueueItem::Remove { current, parent } => match current {
                        TreeNode::Component(component) => {
                            render_queue.queue_cleanups(&component.component);
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
                        TreeNode::Provider(_, children) => {
                            for child in children {
                                render_queue.remove(child, Arc::clone(&parent));
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

fn update_children<N, E>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    mut elements: Vec<Element<N, E>>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    ctx: RenderContext<N>,
) {
    let old_len = tree_nodes.len();

    for tree_node in tree_nodes.drain(elements.len()..).rev() {
        render_queue.remove(tree_node, ctx.parent.clone());
    }

    tree_nodes.shrink_to_fit();

    for element in elements.drain(tree_nodes.len()..) {
        tree_nodes.push(TreeNode::from(element));
    }

    for tree_node in tree_nodes.iter_mut().skip(old_len).rev() {
        render_queue.create(tree_node, ctx.clone());
    }

    let mut sibling = ctx.sibling.clone();
    for (tree_node, element) in tree_nodes.iter_mut().zip(elements.into_iter()).rev() {
        let next_sibling = tree_node.get_first_node();
        render_queue.update(tree_node, element, ctx.with_sibling(sibling));
        sibling = next_sibling;
    }
}

fn render_component<N, E, S>(
    tree_component: &mut TreeComponent<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    signal_sender: &Sender<()>,
    ctx: RenderContext<N>,
    spawner: &S,
) -> Result<(), E>
where
    N: Send + 'static,
    E: Send + 'static,
    S: Spawn,
{
    dbg!("render component");
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

    Ok(match result {
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
            render_queue.queue_effects(&tree_component.component, hook.effects);
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
    })
}

fn replace_node<N, E>(
    node: &mut TreeNode<N, E>,
    element: Element<N, E>,
    render_queue: &mut RenderQueue<N, E, TreeNode<N, E>>,
    ctx: RenderContext<N>,
) {
    let mut old_node = TreeNode::from(element);
    std::mem::swap(node, &mut old_node);
    render_queue.remove(old_node, ctx.parent.clone());
    render_queue.create(node, ctx);
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        hash::Hash,
        sync::{Arc, Mutex},
    };

    use async_channel::{Receiver, RecvError, Sender};
    use async_trait::async_trait;
    use futures_util::{task::Spawn, Future, FutureExt};

    use crate::{use_effect, use_state, Component, Element, ObjectModel};

    struct InnerMockObjectModel {
        created: VecDeque<Arc<MockNode>>,
        updated: VecDeque<Arc<MockNode>>,
        removed: VecDeque<Arc<MockNode>>,
        start_signal: (Sender<()>, Receiver<()>),
        finalize_signal: (Sender<()>, Receiver<()>),
    }

    impl InnerMockObjectModel {
        fn new() -> Arc<Mutex<Self>> {
            Arc::new(Mutex::new(Self {
                created: VecDeque::new(),
                updated: VecDeque::new(),
                removed: VecDeque::new(),
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

    impl ObjectModel for MockObjectModel {
        type Node = MockNode;
        fn create(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            _parent: &std::sync::Arc<Self::Node>,
            _sibling: &Option<std::sync::Arc<Self::Node>>,
        ) {
            println!("create {:?}", node);
            self.0.lock().unwrap().created.push_back(node.clone());
        }

        fn update(
            &mut self,
            _node: &std::sync::Arc<Self::Node>,
            next: &std::sync::Arc<Self::Node>,
        ) {
            self.0.lock().unwrap().updated.push_back(next.clone());
        }

        fn remove(
            &mut self,
            node: &std::sync::Arc<Self::Node>,
            _parent: &std::sync::Arc<Self::Node>,
        ) {
            self.0.lock().unwrap().removed.push_back(node.clone());
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
        let handle = tokio::spawn(async move {
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
    }

    #[tokio::test]
    async fn with_callback() {
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

        let inner_object_model = InnerMockObjectModel::new();
        let object_model = MockObjectModel(inner_object_model.clone());
        let handle = tokio::spawn(async move {
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
    }

    #[tokio::test]
    async fn update_order() {
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
        let handle = tokio::spawn(async move {
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
    }

    #[tokio::test]
    async fn async_component() {
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
        let handle = tokio::spawn(async move {
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
    }

    #[tokio::test]
    async fn with_effect() {
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
                &self.0 as *const Sender<()> == &other.0 as *const Sender<()>
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
        let handle = tokio::spawn(async move {
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
    }
}
