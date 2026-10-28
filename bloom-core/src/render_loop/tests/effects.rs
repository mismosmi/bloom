use super::*;

#[tokio::test]
async fn with_effect() {
    // The render loop isn't Send, so it runs on a LocalSet
    tokio::task::LocalSet::new()
        .run_until(async move {
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
                    std::ptr::eq(&self.0, &other.0)
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
            let handle = tokio::task::spawn_local(async move {
                let root = Arc::new(MockNode(0));
                let element = Element::Component(Arc::new(EffectComponent(MySender(sender))));
                render_loop(root, element, TokioSpawner, object_model)
                    .await
                    .unwrap();
            });

            let render_cycle = inner_object_model.lock().unwrap().render_cycle();
            render_cycle.await;

            assert_eq!(Ok(()), receiver.recv().await);

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn effect_lifecycle() {
    // The render loop isn't Send, so it runs on a LocalSet
    tokio::task::LocalSet::new()
        .run_until(async move {
            type Log = Arc<Mutex<Vec<String>>>;

            /// Effect argument, only the dependency is part of the hash
            #[derive(Clone)]
            struct Probe {
                dependency: i32,
                log: Log,
            }

            impl Hash for Probe {
                fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                    self.dependency.hash(state);
                }
            }

            struct Child {
                dependency: i32,
                render: i32,
                log: Log,
            }

            impl PartialEq for Child {
                fn eq(&self, other: &Self) -> bool {
                    self.dependency == other.dependency && self.render == other.render
                }
            }

            #[async_trait]
            impl Component for Child {
                type Error = ();
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    use_effect(
                        Probe {
                            dependency: self.dependency,
                            log: self.log.clone(),
                        },
                        |probe| {
                            probe
                                .log
                                .lock()
                                .unwrap()
                                .push(format!("effect {}", probe.dependency));
                            // Cleanups don't have to be Send
                            let not_send = std::rc::Rc::new(probe.dependency);
                            move || {
                                probe
                                    .log
                                    .lock()
                                    .unwrap()
                                    .push(format!("cleanup {}", not_send));
                            }
                        },
                    );
                    Ok(Element::Node(MockNode(self.render), Vec::new()))
                }
            }

            struct Parent(Log);

            impl PartialEq for Parent {
                fn eq(&self, _: &Self) -> bool {
                    true
                }
            }

            #[async_trait]
            impl Component for Parent {
                type Error = ();
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    let step = use_state(|| 0i32);
                    if *step < 3 {
                        let step = step.clone();
                        tokio::spawn(async move { step.update(|step| *step + 1) });
                    }
                    // Same dependency, then a new one, then the child is removed
                    let (dependency, render) = match *step {
                        0 => (1, 0),
                        1 => (1, 1),
                        2 => (2, 2),
                        _ => return Ok(().into()),
                    };
                    Ok(Element::Component(Arc::new(Child {
                        dependency,
                        render,
                        log: self.0.clone(),
                    })))
                }
            }

            let log = Log::default();
            let inner_object_model = InnerMockObjectModel::new();
            let object_model = MockObjectModel(inner_object_model.clone());
            let parent_log = log.clone();
            let mut handle = tokio::task::spawn_local(async move {
                let root = Arc::new(MockNode(0));
                let element = Element::Component(Arc::new(Parent(parent_log)));
                render_loop(root, element, TokioSpawner, object_model)
                    .await
                    .unwrap();
            });

            for expected in [
                vec!["effect 1"],
                vec![],
                vec!["cleanup 1", "effect 2"],
                vec!["cleanup 2"],
            ] {
                // Fail instead of waiting forever if the render loop panics
                let render_cycle = inner_object_model.lock().unwrap().render_cycle();
                tokio::select! {
                    _ = render_cycle => {}
                    result = &mut handle => panic!("render loop stopped: {:?}", result),
                }
                let entries: Vec<String> = log.lock().unwrap().drain(..).collect();
                assert_eq!(entries, expected);
            }

            handle.abort();
        })
        .await;
}

#[tokio::test]
async fn commit_runs_cleanups_before_and_effects_after_node_operations() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            type Log = Arc<Mutex<Vec<String>>>;

            /// Effect argument that checks whether the child's node is shown
            #[derive(Clone)]
            struct Probe {
                object_model: Arc<Mutex<InnerMockObjectModel>>,
                root: Arc<MockNode>,
                log: Log,
            }

            impl Hash for Probe {
                fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
            }

            impl Probe {
                fn record(&self, event: &str) {
                    let shown = self
                        .object_model
                        .lock()
                        .unwrap()
                        .children_of(&self.root)
                        .contains(&1);
                    self.log
                        .lock()
                        .unwrap()
                        .push(format!("{} shown: {}", event, shown));
                }
            }

            struct Child(Probe);

            impl PartialEq for Child {
                fn eq(&self, _: &Self) -> bool {
                    true
                }
            }

            #[async_trait]
            impl Component for Child {
                type Error = ();
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    use_effect(self.0.clone(), |probe| {
                        probe.record("effect");
                        move || probe.record("cleanup")
                    });
                    Ok(Element::Node(MockNode(1), Vec::new()))
                }
            }

            struct Parent(Probe);

            impl PartialEq for Parent {
                fn eq(&self, _: &Self) -> bool {
                    true
                }
            }

            #[async_trait]
            impl Component for Parent {
                type Error = ();
                type Node = MockNode;

                async fn render(
                    self: Arc<Self>,
                ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                    let removed = use_state(|| false);
                    if *removed {
                        return Ok(().into());
                    }
                    tokio::spawn(async move { removed.update(|_| true) });
                    Ok(Element::Component(Arc::new(Child(self.0.clone()))))
                }
            }

            let log = Log::default();
            let inner_object_model = InnerMockObjectModel::new();
            let object_model = MockObjectModel(inner_object_model.clone());
            let root = Arc::new(MockNode(0));
            let probe = Probe {
                object_model: inner_object_model.clone(),
                root: root.clone(),
                log: log.clone(),
            };
            let mut handle = tokio::task::spawn_local(async move {
                let element = Element::Component(Arc::new(Parent(probe)));
                render_loop(root, element, TokioSpawner, object_model)
                    .await
                    .unwrap();
            });

            for expected in [vec!["effect shown: true"], vec!["cleanup shown: true"]] {
                next_render_cycle(&inner_object_model, &mut handle).await;
                let entries: Vec<String> = log.lock().unwrap().drain(..).collect();
                assert_eq!(entries, expected);
            }

            handle.abort();
        })
        .await;
}
