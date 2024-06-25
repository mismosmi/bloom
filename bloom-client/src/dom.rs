use std::sync::{Arc, Weak};

use bloom_core::ObjectModel;
use bloom_html::HtmlNode;
use weak_table::PtrWeakKeyHashMap;
use web_sys::{wasm_bindgen::JsCast, window, HtmlElement, Node, Text};

pub(crate) struct Dom {
    node_map: PtrWeakKeyHashMap<Weak<HtmlNode>, Node>,
    hydration_state: Option<PtrWeakKeyHashMap<Weak<HtmlNode>, u32>>,
}

impl Dom {
    pub(crate) fn new() -> Self {
        Self {
            node_map: PtrWeakKeyHashMap::new(),
            hydration_state: None,
        }
    }

    pub(crate) fn hydrate() -> Self {
        Self {
            node_map: PtrWeakKeyHashMap::new(),
            hydration_state: Some(PtrWeakKeyHashMap::new()),
        }
    }

    pub(crate) fn register(&mut self, node: &Arc<HtmlNode>, web_node: &Node) {
        self.node_map.insert(node.clone(), web_node.clone());
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
        if let Some(hydration_state) = &mut self.hydration_state {
            let parent_node = self.node_map.get(parent).expect("Parent not found");
            let current_index = hydration_state.get(parent).cloned().unwrap_or_default();

            let current_node = parent_node
                .child_nodes()
                .item(current_index)
                .expect("Hydration Error");
            self.node_map.insert(node.clone(), current_node);
            hydration_state.insert(parent.clone(), current_index + 1);
            return;
        }

        let new_node = match node.as_ref() {
            HtmlNode::Element(element) => Self::create_element(element),
            HtmlNode::Text(text) => {
                let text_node = Self::document().create_text_node(text);
                text_node.set_text_content(Some(text));
                text_node.into()
            }
        };
        self.node_map.insert(node.clone(), new_node.clone());

        let parent_node = self.node_map.get(parent).expect("Parent not found");
        let sibling_node = sibling
            .as_ref()
            .map(|sibling| self.node_map.get(&sibling).expect("Sibling not found"));

        parent_node
            .insert_before(&new_node, sibling_node)
            .expect("Failed to insert node");
    }

    fn remove(&mut self, node: &std::sync::Arc<Self::Node>, parent: &std::sync::Arc<Self::Node>) {
        let parent_node = self.node_map.get(parent).expect("Parent not found");
        let current_node = self.node_map.get(node).expect("Node not found");
        parent_node
            .remove_child(current_node)
            .expect("Failed to remove child node");
    }

    fn update(&mut self, node: &std::sync::Arc<Self::Node>, next: &std::sync::Arc<Self::Node>) {
        let current_node = self.node_map.get(node).expect("Node not found");

        let current_node = match next.as_ref() {
            HtmlNode::Element(element) => {
                if let Some(current_element) = current_node.dyn_ref::<HtmlElement>() {
                    if current_element.tag_name() != element.tag_name() {
                        let new_node = Self::create_element(element);
                        current_node
                            .parent_node()
                            .expect("Failed to get parent node")
                            .replace_child(&new_node, current_node)
                            .expect("Failed to replace child node");

                        Some(new_node)
                    } else {
                        for (key, value) in element.attributes() {
                            current_element
                                .set_attribute(key, value)
                                .expect("Failed to set attribute");
                        }

                        let attributes = current_element.attributes();
                        for index in 0..attributes.length() {
                            if let Some(attr) = attributes.item(index) {
                                let name = attr.name();
                                if !element.attributes().contains_key(&name) {
                                    current_element
                                        .remove_attribute(&name)
                                        .expect("Failed to remove attribute");
                                }
                            }
                        }

                        None
                    }
                } else {
                    let new_node = Self::create_element(element);
                    current_node
                        .parent_node()
                        .expect("Failed to get parent node")
                        .replace_child(&new_node, current_node)
                        .expect("Failed to replace child node");

                    Some(new_node)
                }
            }
            HtmlNode::Text(text) => {
                if let Some(current_text_node) = current_node.dyn_ref::<Text>() {
                    current_text_node.set_text_content(Some(text));
                    None
                } else {
                    let new_node = Self::document().create_text_node(text).into();
                    current_node
                        .parent_node()
                        .expect("Failed to get parent node")
                        .replace_child(&new_node, current_node)
                        .expect("Failed to replace child node");

                    Some(new_node)
                }
            }
        };

        if let Some(new_node) = current_node {
            self.node_map.insert(next.clone(), new_node);
        }
    }

    fn finalize(&mut self) {
        self.hydration_state = None;
    }
}

impl Dom {
    fn document() -> web_sys::Document {
        window()
            .expect("Window not found")
            .document()
            .expect("Document not found")
    }

    fn create_element(element: &bloom_html::HtmlElement) -> Node {
        let new_node = Self::document()
            .create_element(element.tag_name())
            .expect("Element not created");

        for (key, value) in element.attributes() {
            new_node
                .set_attribute(key, value)
                .expect("Failed to set attribute");
        }

        new_node.into()
    }
}
