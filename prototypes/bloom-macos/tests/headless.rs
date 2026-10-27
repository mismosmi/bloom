//! Drives bloom-core's render_loop with the native node type and a recording
//! ObjectModel, to show rsx + diffing work with a non-HTML node type.
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bloom_core::{render_loop, use_state, Component, Element, ObjectModel};
use bloom_macos::node::{prelude::*, NativeEvent, NativeNode};
use bloom_rsx::rsx;

type Log = Arc<Mutex<Vec<String>>>;

struct Recorder {
    log: Log,
    clicks: Arc<Mutex<Vec<Arc<NativeNode>>>>,
    signal: Option<async_channel::Sender<()>>,
    frames: usize,
}

fn describe(node: &NativeNode) -> String {
    match node {
        NativeNode::Element(e) => e.tag().to_string(),
        NativeNode::Text(t) => format!("{t:?}"),
    }
}

impl ObjectModel for Recorder {
    type Node = NativeNode;

    fn create(
        &mut self,
        node: &Arc<NativeNode>,
        parent: &Arc<NativeNode>,
        _: &Option<Arc<NativeNode>>,
    ) {
        if let NativeNode::Element(e) = node.as_ref() {
            if e.callbacks().contains_key("click") {
                self.clicks.lock().unwrap().push(node.clone());
            }
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("create {} in {}", describe(node), describe(parent)));
    }

    fn remove(&mut self, node: &Arc<NativeNode>, _: &Arc<NativeNode>) {
        self.log
            .lock()
            .unwrap()
            .push(format!("remove {}", describe(node)));
    }

    fn update(&mut self, node: &Arc<NativeNode>, next: &Arc<NativeNode>) {
        if let NativeNode::Element(e) = next.as_ref() {
            if e.callbacks().contains_key("click") {
                self.clicks.lock().unwrap().push(next.clone());
            }
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("update {} -> {}", describe(node), describe(next)));
    }

    fn subscribe(&mut self, signal: async_channel::Sender<()>) {
        self.signal = Some(signal);
    }

    async fn finalize(&mut self) {
        self.frames += 1;
        if self.frames == 1 {
            // simulate the user pressing the button
            let node = self.clicks.lock().unwrap().last().cloned().unwrap();
            if let NativeNode::Element(e) = node.as_ref() {
                (e.callbacks()["click"])(NativeEvent::Click);
            }
        } else if let Some(signal) = self.signal.take() {
            signal.close();
        }
    }
}

#[derive(PartialEq)]
struct Counter;

#[async_trait]
impl Component for Counter {
    type Node = NativeNode;
    type Error = ();

    async fn render(self: Arc<Self>) -> Result<Element<NativeNode, ()>, ()> {
        let count = use_state(|| 0);
        Ok(rsx!(
            <stack direction="column" gap=8>
                <label>{format!("Count: {}", *count)}</label>
                <button title="Increment" on_click={move |_| count.update(|c| *c + 1)} />
            </stack>
        ))
    }
}

#[test]
fn counter_renders_and_rerenders() {
    let log: Log = Arc::default();
    let model = Recorder {
        log: log.clone(),
        clicks: Arc::default(),
        signal: None,
        frames: 0,
    };
    let root = Arc::new(NativeNode::Element(Arc::new(tag("window").build())));
    let pool = futures_executor::ThreadPool::new().unwrap();
    futures_executor::block_on(render_loop(root, Element::from(Counter), pool, model)).unwrap();

    let log = log.lock().unwrap();
    for line in log.iter() {
        println!("{line}");
    }
    assert!(log.contains(&"create \"Count: 0\" in label".to_string()));
    assert!(log.contains(&"update \"Count: 0\" -> \"Count: 1\"".to_string()));
}
