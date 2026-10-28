use super::*;

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
                render_loop(root, element, spawner, object_model)
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
