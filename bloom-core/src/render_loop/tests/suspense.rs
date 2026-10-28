use super::*;

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
        render_loop(root, element, TokioSpawner, object_model)
            .await
            .unwrap();
    });
    (inner_object_model, handle)
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
