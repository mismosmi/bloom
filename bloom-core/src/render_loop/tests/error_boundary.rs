use super::*;

fn node(value: i32) -> Element<MockNode, i32> {
    Element::Node(MockNode(value), Vec::new())
}

fn boundary(children: Vec<Element<MockNode, i32>>) -> Element<MockNode, i32> {
    ErrorBoundary::builder()
        .fallback(|error| node(100 + *error))
        .children(children)
        .build()
        .into()
}

/// Fails with its value from the given step of its parent on, renders its node before
struct FailAt {
    value: i32,
    step: i32,
    fail_at: i32,
}

impl PartialEq for FailAt {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.step == other.step && self.fail_at == other.fail_at
    }
}

#[async_trait]
impl Component for FailAt {
    type Error = i32;
    type Node = MockNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        if self.step >= self.fail_at {
            return Err(self.value);
        }
        Ok(node(self.value))
    }
}

fn fail_at(value: i32, step: i32, fail_at: i32) -> Element<MockNode, i32> {
    Element::Component(Arc::new(FailAt {
        value,
        step,
        fail_at,
    }))
}

/// Advances its step on every render cycle until the last one and renders the element of the step
struct Stepper {
    last: i32,
    render: fn(i32) -> Element<MockNode, i32>,
}

impl PartialEq for Stepper {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[async_trait]
impl Component for Stepper {
    type Error = i32;
    type Node = MockNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        let step = use_state(|| 0i32);
        if *step < self.last {
            let step = step.clone();
            tokio::spawn(async move { step.update(|step| *step + 1) });
        }
        Ok((self.render)(*step))
    }
}

fn stepper(last: i32, render: fn(i32) -> Element<MockNode, i32>) -> Element<MockNode, i32> {
    Element::Component(Arc::new(Stepper { last, render }))
}

type Handle = tokio::task::JoinHandle<Result<(), i32>>;

fn start_render_loop(
    element: Element<MockNode, i32>,
) -> (Arc<Mutex<InnerMockObjectModel>>, Arc<MockNode>, Handle) {
    let inner_object_model = InnerMockObjectModel::new();
    let object_model = MockObjectModel(inner_object_model.clone());
    let root = Arc::new(MockNode(0));
    let loop_root = root.clone();
    // The render loop isn't Send, so it runs on a LocalSet
    let handle = tokio::task::spawn_local(async move {
        render_loop(loop_root, element, TokioSpawner, object_model).await
    });
    (inner_object_model, root, handle)
}

async fn next_cycle(inner_object_model: &Arc<Mutex<InnerMockObjectModel>>, handle: &mut Handle) {
    let render_cycle = inner_object_model.lock().unwrap().render_cycle();
    tokio::select! {
        _ = render_cycle => {}
        result = handle => panic!("render loop stopped: {:?}", result),
    }
}

#[tokio::test]
async fn shows_fallback_on_first_render() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let element = Element::Fragment(vec![
                node(1),
                boundary(vec![node(2), fail_at(3, 0, 0), node(4)]),
                node(5),
            ]);
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            {
                let mut lock = inner.lock().unwrap();
                // The siblings of the failed child are created and removed in the same commit
                lock.assert_children(&root, &[1, 103, 5], &[1, 2, 4, 5, 103], &[2, 4], 0);
            }

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn replaces_children_when_a_child_fails_later() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            static CLEANUPS: AtomicUsize = AtomicUsize::new(0);
            static EFFECTS: AtomicUsize = AtomicUsize::new(0);

            #[derive(PartialEq)]
            struct WithEffect(i32);

            #[async_trait]
            impl Component for WithEffect {
                type Error = i32;
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    use_effect(self.0, |_| {
                        EFFECTS.fetch_add(1, Ordering::SeqCst);
                        || {
                            CLEANUPS.fetch_add(1, Ordering::SeqCst);
                        }
                    });
                    Ok(node(2))
                }
            }

            let element = Element::Fragment(vec![
                node(1),
                stepper(1, |step| {
                    boundary(vec![
                        Element::Component(Arc::new(WithEffect(step))),
                        Element::Node(MockNode(3), vec![fail_at(4, step, 1)]),
                    ])
                }),
                node(5),
            ]);
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            {
                let mut lock = inner.lock().unwrap();
                lock.assert_children(&root, &[1, 2, 3, 5], &[1, 2, 3, 4, 5], &[], 0);
            }
            assert_eq!(EFFECTS.load(Ordering::SeqCst), 1);

            next_cycle(&inner, &mut handle).await;
            {
                let mut lock = inner.lock().unwrap();
                lock.assert_children(&root, &[1, 104, 5], &[104], &[2, 3, 4], 0);
            }
            // The effect of the failed render pass doesn't run, the previous one is cleaned up
            assert_eq!(EFFECTS.load(Ordering::SeqCst), 1);
            assert_eq!(CLEANUPS.load(Ordering::SeqCst), 1);

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn keeps_fallback_until_created_again() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let element = stepper(2, |step| match step {
                0 => keyed(0, boundary(vec![fail_at(1, 0, 0)])),
                1 => keyed(0, boundary(vec![node(2)])),
                _ => keyed(1, boundary(vec![node(3)])),
            });
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[101], &[101], &[], 0);

            // The child would render now, but the boundary ignores its children
            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[101], &[], &[], 0);

            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[3], &[3], &[101], 0);

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn nested_boundaries() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            // The fallback of the inner boundary fails too, so the outer boundary catches it
            let inner_boundary = ErrorBoundary::builder()
                .fallback(|error| fail_at(*error + 10, 0, 0))
                .children(vec![fail_at(1, 0, 0)])
                .build()
                .into();
            let element = boundary(vec![
                node(2),
                inner_boundary,
                boundary(vec![fail_at(3, 0, 0)]),
            ]);
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[111], &[2, 103, 111], &[2, 103], 0);

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn catches_errors_of_suspended_components() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (sender, receiver) = async_channel::bounded::<()>(1);

            struct FailLater(Receiver<()>);

            impl PartialEq for FailLater {
                fn eq(&self, _: &Self) -> bool {
                    true
                }
            }

            #[async_trait]
            impl Component for FailLater {
                type Error = i32;
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    self.0.recv().await.unwrap();
                    Err(2)
                }
            }

            let element = boundary(vec![
                Suspense::builder()
                    .fallback(node(9))
                    .children(vec![
                        node(1),
                        Element::Component(Arc::new(FailLater(receiver))),
                    ])
                    .build()
                    .into(),
                node(3),
            ]);
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[9, 3], &[3, 9], &[], 0);

            sender.send(()).await.unwrap();
            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[102], &[102], &[3, 9], 0);

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn error_without_boundary_stops_the_render_loop() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (_inner, _root, handle) =
                start_render_loop(Element::Fragment(vec![node(1), fail_at(2, 0, 0)]));
            assert_eq!(handle.await.unwrap(), Err(2));
        })
        .await;
}

/// Fails while the flag is set
struct FailIf(Arc<AtomicBool>, i32);

impl PartialEq for FailIf {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[async_trait]
impl Component for FailIf {
    type Error = i32;
    type Node = MockNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        if self.0.load(Ordering::SeqCst) {
            return Err(self.1);
        }
        Ok(node(self.1))
    }
}

/// Renders a boundary around a child that fails while the flag is set
/// and passes the caught errors to the sender
struct Owner {
    fail: Arc<AtomicBool>,
    errors: Sender<CaughtError<i32>>,
}

impl PartialEq for Owner {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[async_trait]
impl Component for Owner {
    type Error = i32;
    type Node = MockNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        let errors = self.errors.clone();
        Ok(Element::Fragment(vec![
            ErrorBoundary::builder()
                .fallback(move |error: CaughtError<i32>| {
                    let value = 100 + *error;
                    errors.try_send(error).unwrap();
                    node(value)
                })
                .children(vec![
                    node(1),
                    Element::Component(Arc::new(FailIf(self.fail.clone(), 2))),
                ])
                .build()
                .into(),
            node(3),
        ]))
    }
}

#[tokio::test]
async fn dismiss_shows_children_again() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let fail = Arc::new(AtomicBool::new(true));
            let (errors, caught) = async_channel::unbounded();
            let element = Element::Component(Arc::new(Owner {
                fail: fail.clone(),
                errors,
            }));
            let (inner, root, mut handle) = start_render_loop(element);

            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[102, 3], &[1, 3, 102], &[1], 0);
            let error = caught.try_recv().unwrap();
            assert_eq!(*error, 2);

            // Dismissing while the child still fails shows a new fallback
            error.dismiss();
            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[102, 3], &[1, 102], &[1, 102], 0);
            let error = caught.try_recv().unwrap();

            fail.store(false, Ordering::SeqCst);
            error.dismiss();
            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[1, 2, 3], &[1, 2], &[102], 0);
            assert!(caught.try_recv().is_err());

            // A stale error doesn't affect the children
            error.dismiss();
            next_cycle(&inner, &mut handle).await;
            inner
                .lock()
                .unwrap()
                .assert_children(&root, &[1, 2, 3], &[], &[], 0);

            handle.abort();
        })
        .await;
}
