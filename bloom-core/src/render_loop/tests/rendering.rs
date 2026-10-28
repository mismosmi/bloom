use super::*;

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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(root, element, TokioSpawner, object_model)
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
                render_loop(loop_root, element, spawner, object_model)
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
