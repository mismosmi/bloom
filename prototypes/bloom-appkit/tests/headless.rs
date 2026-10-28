//! Drives bloom-core's render_loop with AppKitNode and a recording ObjectModel.
//! No AppKit objects are created, so this runs off the main thread.
#![cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bloom_appkit::{AppKitNode, Button, StackView, Window};
use bloom_core::{render_loop, use_state, Component, Element, ObjectModel};
use bloom_rsx::rsx;

fn describe(node: &AppKitNode) -> String {
    match node {
        AppKitNode::Application => "Application".into(),
        AppKitNode::Window(_) => "Window".into(),
        AppKitNode::StackView(_) => "StackView".into(),
        AppKitNode::Button(b) => format!("Button({:?})", b.title.as_deref().unwrap_or("")),
        AppKitNode::TextField(t) => {
            format!("TextField({:?})", t.string_value.as_deref().unwrap_or(""))
        }
    }
}

#[derive(Default)]
struct Recorder {
    log: Arc<Mutex<Vec<String>>>,
    button: Arc<Mutex<Option<Arc<AppKitNode>>>>,
    signal: Option<async_channel::Sender<()>>,
}

impl ObjectModel for Recorder {
    type Node = AppKitNode;

    fn create(
        &mut self,
        node: &Arc<AppKitNode>,
        parent: &Arc<AppKitNode>,
        _: &Option<Arc<AppKitNode>>,
    ) {
        if let AppKitNode::Button(_) = node.as_ref() {
            *self.button.lock().unwrap() = Some(node.clone());
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("create {} in {}", describe(node), describe(parent)));
    }

    fn remove(&mut self, node: &Arc<AppKitNode>, _: &Arc<AppKitNode>) {
        self.log
            .lock()
            .unwrap()
            .push(format!("remove {}", describe(node)));
    }

    fn update(&mut self, node: &Arc<AppKitNode>, next: &Arc<AppKitNode>) {
        self.log
            .lock()
            .unwrap()
            .push(format!("update {} -> {}", describe(node), describe(next)));
    }

    fn subscribe(&mut self, signal: async_channel::Sender<()>) {
        self.signal = Some(signal);
    }

    async fn finalize(&mut self) {
        // one pass is enough: handlers take an NSButton, so clicks need AppKit
        if let Some(signal) = self.signal.take() {
            signal.close();
        }
    }
}

#[derive(PartialEq)]
struct Counter;

#[async_trait]
impl Component for Counter {
    type Node = AppKitNode;
    type Error = ();

    async fn render(self: Arc<Self>) -> Result<Element<AppKitNode, ()>, ()> {
        let count = use_state(|| 0);
        Ok(rsx!(
            <Window title="Counter">
                <StackView spacing=8>
                    {format!("Count: {}", *count)}
                    <Button title="Increment" on_action={move |_| count.update(|c| *c + 1)} />
                </StackView>
            </Window>
        ))
    }
}

#[test]
fn counter_renders_typed_nodes() {
    let model = Recorder::default();
    let log = model.log.clone();
    let button = model.button.clone();
    let root = Arc::new(AppKitNode::Application);
    let pool = futures_executor::ThreadPool::new().unwrap();
    futures_executor::block_on(render_loop(root, Element::from(Counter), pool, model)).unwrap();

    let log = log.lock().unwrap();
    assert!(
        log.contains(&"create Window in Application".to_string()),
        "{log:#?}"
    );
    assert!(
        log.contains(&"create StackView in Window".to_string()),
        "{log:#?}"
    );
    assert!(
        log.contains(&"create TextField(\"Count: 0\") in StackView".to_string()),
        "{log:#?}"
    );
    assert!(
        log.contains(&"create Button(\"Increment\") in StackView".to_string()),
        "{log:#?}"
    );
    let button = button.lock().unwrap().clone().unwrap();
    let AppKitNode::Button(button) = button.as_ref() else {
        unreachable!()
    };
    assert!(button.on_action.is_some());
}
