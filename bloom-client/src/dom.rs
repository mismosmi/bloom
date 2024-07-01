use std::{
    collections::HashMap,
    future::poll_fn,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    task::Poll,
};

use bloom_core::ObjectModel;
use bloom_html::HtmlNode;
use futures_util::Future;
use weak_table::PtrWeakKeyHashMap;
use web_sys::{
    console,
    wasm_bindgen::{closure::Closure, JsCast},
    window, Element, Node, Text,
};

fn document() -> web_sys::Document {
    window()
        .expect("Window not found")
        .document()
        .expect("Document not found")
}

enum NodeState {
    Element {
        node: Element,
        callbacks: HashMap<String, Closure<dyn Fn(web_sys::Event)>>,
    },
    Text {
        node: Text,
    },
}

impl NodeState {
    fn create(node: &Arc<HtmlNode>) -> Self {
        match node.as_ref() {
            HtmlNode::Element(element) => {
                let dom_node = document()
                    .create_element(element.tag_name())
                    .expect("Element not created");

                for (key, value) in element.attributes() {
                    dom_node
                        .set_attribute(key, value)
                        .expect("Failed to set attribute");
                }

                if let Some(dom_ref) = element.dom_ref() {
                    dom_ref.set(dom_node.clone());
                }

                Self::Element {
                    callbacks: Self::setup_callbacks(node, &dom_node),
                    node: dom_node,
                }
            }
            HtmlNode::Text(text) => {
                let text_node = document().create_text_node(text);
                text_node.set_text_content(Some(text));
                Self::Text { node: text_node }
            }
        }
    }

    fn hydrate(node: &Arc<HtmlNode>, dom_node: Node) -> Self {
        match node.as_ref() {
            HtmlNode::Element(element) => {
                let dom_node: web_sys::Element = dom_node
                    .dyn_into()
                    .expect("Expected Element, received Text");

                if let Some(dom_ref) = element.dom_ref() {
                    dom_ref.set(dom_node.clone());
                }

                Self::Element {
                    callbacks: Self::setup_callbacks(
                        node,
                        dom_node.dyn_ref().expect("Expected Element, received Text"),
                    ),
                    node: dom_node,
                }
            }
            HtmlNode::Text(_) => Self::Text {
                node: dom_node
                    .dyn_into()
                    .expect("Expected Text, received Element"),
            },
        }
    }

    fn setup_callbacks(
        node: &Arc<HtmlNode>,
        dom_node: &Element,
    ) -> HashMap<String, Closure<dyn Fn(web_sys::Event)>> {
        let mut registered_callbacks = HashMap::new();
        for key in node
            .as_element()
            .expect("Cannot setup callbacks for text node")
            .callbacks()
            .keys()
        {
            let node = node.clone();
            let cloned_key = key.clone();
            let closure: Closure<dyn Fn(web_sys::Event)> =
                Closure::new(move |event: web_sys::Event| {
                    if let Some(callback) = node
                        .as_element()
                        .and_then(|el| el.callbacks().get(&cloned_key))
                    {
                        callback(event);
                    }
                });
            dom_node
                .add_event_listener_with_callback(key, closure.as_ref().unchecked_ref())
                .expect("Failed to add event listener");
            registered_callbacks.insert(key.clone(), closure);
        }
        registered_callbacks
    }

    fn clear_callbacks(self) -> Node {
        match self {
            Self::Element { node, callbacks } => {
                for (key, closure) in callbacks.into_iter() {
                    node.remove_event_listener_with_callback(
                        &key,
                        closure.as_ref().unchecked_ref(),
                    )
                    .expect("Failed to remove event listener");
                }
                node.into()
            }
            Self::Text { node } => node.into(),
        }
    }

    fn node(&self) -> &Node {
        match self {
            Self::Element { node, .. } => node,
            Self::Text { node } => node,
        }
    }
}

pub(crate) struct Dom {
    nodes: PtrWeakKeyHashMap<Weak<HtmlNode>, NodeState>,
    hydration_state: Option<PtrWeakKeyHashMap<Weak<HtmlNode>, u32>>,
}

impl Dom {
    pub(crate) fn new() -> Self {
        Self {
            nodes: PtrWeakKeyHashMap::new(),
            hydration_state: None,
        }
    }

    pub(crate) fn hydrate() -> Self {
        Self {
            nodes: PtrWeakKeyHashMap::new(),
            hydration_state: Some(PtrWeakKeyHashMap::new()),
        }
    }

    pub(crate) fn register(&mut self, node: &Arc<HtmlNode>, dom_node: Node) {
        self.nodes
            .insert(node.clone(), NodeState::hydrate(node, dom_node));
    }
}

impl ObjectModel for Dom {
    type Node = HtmlNode;

    fn create(
        &mut self,
        node: &std::sync::Arc<Self::Node>,
        parent: &std::sync::Arc<Self::Node>,
        sibling: &Option<std::sync::Arc<Self::Node>>,
    ) {
        console::log_1(&format!("Create {:?}", node).into());
        let parent_state = self.nodes.get(parent).expect("Parent not found");

        if let Some(hydration_state) = &mut self.hydration_state {
            console::log_1(&"Hydrate".into());
            let hydration_index = hydration_state.get(parent).cloned().unwrap_or(0);

            let existing_node = parent_state
                .node()
                .child_nodes()
                .item(hydration_index)
                .expect("Hydration mismatch");

            hydration_state.insert(parent.clone(), hydration_index + 1);
            self.nodes
                .insert(node.clone(), NodeState::hydrate(node, existing_node));
            return;
        }

        let sibling_node = sibling
            .as_ref()
            .map(|sibling| self.nodes.get(&sibling).expect("Sibling not found").node());

        let state = NodeState::create(node);

        parent_state
            .node()
            .insert_before(state.node(), sibling_node)
            .expect("Failed to insert node");

        self.nodes.insert(node.clone(), state);
    }

    fn remove(&mut self, node: &std::sync::Arc<Self::Node>, parent: &std::sync::Arc<Self::Node>) {
        let parent_node = self.nodes.get(parent).expect("Parent not found").node();
        let current_node = self.nodes.get(node).expect("Node not found").node();
        parent_node
            .remove_child(current_node)
            .expect("Failed to remove child node");
    }

    fn update(&mut self, node: &std::sync::Arc<Self::Node>, next: &std::sync::Arc<Self::Node>) {
        let current_state = self.nodes.remove(node).expect("Node not found");
        let current_node = current_state.clear_callbacks();

        match next.as_ref() {
            HtmlNode::Element(element) => {
                if let Some(current_element) = current_node.dyn_ref::<web_sys::HtmlElement>() {
                    if current_element.tag_name().to_lowercase() != element.tag_name() {
                        console::log_1(
                            &format!(
                                "Replace tag {} -> {}",
                                current_element.tag_name(),
                                element.tag_name()
                            )
                            .into(),
                        );
                        let new_state = NodeState::create(next);

                        current_element
                            .parent_node()
                            .expect("Failed to get parent node")
                            .replace_child(new_state.node(), current_element)
                            .expect("Failed to replace child node");

                        self.nodes.insert(next.clone(), new_state);
                    } else {
                        console::log_1(&format!("Update tag {}", element.tag_name()).into());
                        for (key, value) in element.attributes() {
                            current_element
                                .set_attribute(key, value)
                                .expect("Failed to set attribute");
                        }

                        for name in current_element.get_attribute_names() {
                            let name = name.as_string().expect("Attribute name is not a string");
                            if !element.attributes().contains_key(&name) {
                                current_element
                                    .remove_attribute(&name)
                                    .expect("Failed to remove attribute");
                            }
                        }

                        self.nodes
                            .insert(next.clone(), NodeState::hydrate(node, current_node));
                        console::log_1(&format!("Updated tag {}", element.tag_name()).into());
                    }
                } else {
                    console::log_1(&format!("Replace {:?} -> {:?}", current_node, next).into());
                    let new_state = NodeState::create(next);

                    current_node
                        .parent_node()
                        .expect("Failed to get parent node")
                        .replace_child(new_state.node(), &current_node)
                        .expect("Failed to replace child node");

                    self.nodes.insert(next.clone(), new_state);
                }
            }
            HtmlNode::Text(text) => {
                if let Some(current_text_node) = current_node.dyn_ref::<Text>() {
                    console::log_1(&format!("Update text {}", text).into());
                    if current_text_node.text_content().as_ref() != Some(text) {
                        current_text_node.set_text_content(Some(text));
                    }
                    self.nodes
                        .insert(next.clone(), NodeState::hydrate(node, current_node));
                } else {
                    console::log_1(&format!("Replace text {}", text).into());
                    let new_state = NodeState::create(next);

                    current_node
                        .parent_node()
                        .expect("Failed to get parent node")
                        .replace_child(new_state.node(), &current_node)
                        .expect("Failed to replace child node");

                    self.nodes.insert(next.clone(), new_state);
                }
            }
        }
    }

    fn finalize(&mut self) -> impl Future<Output = ()> {
        console::log_1(&"Finalize".into());
        self.hydration_state = None;
        let ready = Arc::new(AtomicBool::new(false));

        poll_fn(move |cx| {
            if ready.load(Ordering::Relaxed) {
                Poll::Ready(())
            } else {
                let waker = cx.waker().clone();
                let ready = ready.clone();
                let cb = Closure::once_into_js(move || {
                    ready.store(true, Ordering::Relaxed);
                    waker.wake();
                });
                window()
                    .expect("Window not found")
                    .request_animation_frame(cb.dyn_ref().expect("Failed to cast callback"))
                    .expect("Failed to request animation frame");

                Poll::Pending
            }
        })
    }
}

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests {
    use bloom_html::tag::div;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::HtmlElement;

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn update_node() {
        let mut dom = Dom::new();

        let dom_root: web_sys::Node = document().create_element("div").unwrap().into();
        let root = Arc::new(div().into());
        dom.register(&root, dom_root.clone());
        let node = Arc::new(div().into());
        dom.create(&node, &root, &None);
        let text = Arc::new(HtmlNode::text("0".to_string()));
        dom.create(&text, &node, &None);
        dom.finalize();

        assert_eq!(dom_root.child_nodes().length(), 1);
        let dom_node = dom_root.child_nodes().item(0).unwrap();
        assert_eq!(dom_node.text_content().unwrap(), "0");

        let next = Arc::new(div().into());
        dom.update(&node, &next);
        let next_text = Arc::new(HtmlNode::text("1".to_string()));
        dom.update(&text, &next_text);
        dom.finalize();

        assert_eq!(
            dom_root.child_nodes().item(0).unwrap(),
            dom_node,
            "Node should not change"
        );
        assert_eq!(dom_node.text_content().unwrap(), "1");

        dom.remove(&next, &root);
        assert_eq!(dom_root.child_nodes().length(), 0);
    }
}
