use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use bloom_core::{use_effect, use_ref, use_state, Component, Element};
use bloom_html::{DomRef, HtmlNode};
use web_sys::{js_sys::Array, wasm_bindgen::{intern, JsCast}};

use crate::interned_str::interned;

pub struct ServerBoundary<E>{
    _pd: PhantomData<E>,
    root: Arc<DomRef>
}

impl<E> PartialEq for ServerBoundary<E> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<E> ServerBoundary<E> {
    pub fn new() -> ServerBoundaryBuilder<()> {
        ServerBoundaryBuilder {
            root: ()
        }
    }
}

struct ServerBoundaryBuilder<T> {
    root: T
}

impl ServerBoundaryBuilder<()> {
    pub fn root(self, root: Arc<HtmlNode>) -> ServerBoundaryBuilder<Arc<HtmlNode>> {
        ServerBoundaryBuilder { root }
    }
}

impl ServerBoundaryBuilder<Arc<DomRef>> {
    pub fn build<E>(self) -> ServerBoundary<E> {
        ServerBoundary {
            _pd: PhantomData,
            root: self.root
        }
    }
}

fn build_node(node: &web_sys::Node) -> HtmlNode {
    if let Some(element_node) = node.dyn_ref::<web_sys::Element>() {
            let mut node_builder = HtmlNode::element(interned(element_node.tag_name()));

            for key in element_node.get_attribute_names() {
                let key = key.as_string().expect("Attribute name is not a string");
                let value = element_node.get_attribute(&key).expect("Attribute value is not a string");
                node_builder = node_builder.attr(key, value);
            }

            return node_builder.build().into()
    }

    if let Some(text_node) = node.dyn_ref::<web_sys::Text>() {
        return HtmlNode::text(text_node.text_content().unwrap_or_default())
    }

    if let Some(comment_node) = node.dyn_ref::<web_sys::Comment>() {
        return HtmlNode::comment(comment_node.text_content().unwrap_or_default()).build().into()
    }

    panic!("Unexpected Node Type");
}

fn build_tree<E>(root: Arc<DomRef>) {
    let mut dom_stack: Vec<(Array, Vec<HtmlNode>)> = vec![
        (Array::from(&root.get().expect("Root node is not ready").child_nodes().into()), Vec::new())
    ];

    while let Some((dom_nodes, nodes)) = stack.last_mut() {
        let dom_node = dom_nodes.shift();

        if dom_node.is_undefined() {

        }
    }
}

#[async_trait]
impl<E> Component for ServerBoundary<E>
where
    E: Send + Sync,
{
    type Node = HtmlNode;
    type Error = E;

    async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, E>, E> {
        let state = use_state(|| 0i16);


        use_effect((start_comment, state), |(start_comment, state)| {
            let start_node = if let Some(start_comment) = start_comment.get() {
                start_comment
            } else {
                return;
            };

            let stack = vec![start_node];

            while let Some(next_node)

        });

        Ok(vec![HtmlNode::comment("bloom-sb-start".to_string())
            .dom_ref(start_comment)
            .build()
            .into()]
        .into())
    }
}
