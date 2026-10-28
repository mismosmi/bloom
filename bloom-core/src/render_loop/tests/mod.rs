//! The render loop is tested against a mock object model that tracks the children of every node.

mod effects;
mod error_boundary;
mod keyed;
mod nesting;
mod rendering;
mod suspense;

use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use async_channel::{Receiver, RecvError, Sender};
use async_trait::async_trait;
use futures_util::{task::Spawn, Future, FutureExt};

use crate::{
    keyed, use_effect, use_ref_with_default, use_state, CaughtError, Component, Element,
    ErrorBoundary, Fragment, ObjectModel, Provider, Suspense,
};

use super::render_loop;

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

    fn remove(&mut self, node: &std::sync::Arc<Self::Node>, parent: &std::sync::Arc<Self::Node>) {
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

#[derive(Clone)]
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
            render_loop(loop_root, element, TokioSpawner, object_model)
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
