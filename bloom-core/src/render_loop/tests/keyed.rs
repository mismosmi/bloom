use super::*;

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
            let mut harness = StepsHarness::start(vec![vec![-1, 1, 2], vec![2, -2, 1]], |ids| {
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
