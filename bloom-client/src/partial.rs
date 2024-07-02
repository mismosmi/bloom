use async_channel::Sender;
use bloom_core::{render_loop, Element, ObjectModel};
use bloom_html::HtmlNode;
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    fmt::Debug,
    sync::Arc,
};
use wasm_bindgen_futures::spawn_local;
use web_sys::{console, js_sys::Array, window, Node};

use crate::{dom::Dom, interned_str::interned, spawner::WasmSpawner};

#[derive(Default)]
struct PartialRenderingContext {
    context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    subscribers: Vec<Sender<()>>,
}

impl Drop for PartialRenderingContext {
    fn drop(&mut self) {
        for subscriber in self.subscribers.drain(..) {
            subscriber.close();
        }
    }
}

thread_local! {
    static CONTEXT: RefCell<HashMap<u64, PartialRenderingContext>> = RefCell::new(HashMap::new());
}

struct PartialDom(Dom, u64);

impl PartialDom {
    fn hydrate_from(
        context_id: u64,
        root: Arc<HtmlNode>,
        dom_node: Node,
        start_index: i32,
    ) -> Self {
        let mut inner = Dom::hydrate();
        inner.register(&root, dom_node);
        inner.set_hydration_index(root, start_index.unsigned_abs());
        Self(inner, context_id)
    }
}

impl ObjectModel for PartialDom {
    type Node = HtmlNode;

    fn create(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    ) {
        self.0.create(node, parent, sibling)
    }

    fn update(&mut self, node: &Arc<Self::Node>, next: &Arc<Self::Node>) {
        self.0.update(node, next)
    }

    fn remove(&mut self, node: &Arc<Self::Node>, parent: &Arc<Self::Node>) {
        self.0.remove(node, parent)
    }

    fn finalize(&mut self) -> impl futures_util::Future<Output = ()> + Send {
        self.0.finalize()
    }

    fn subscribe(&mut self, signal: Sender<()>) {
        CONTEXT.with(|context| {
            let mut context = context.borrow_mut();
            let context = context
                .entry(self.1)
                .or_insert_with(|| PartialRenderingContext::default());
            context.subscribers.push(signal);
        });
    }

    fn get_context(&mut self) -> Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>> {
        CONTEXT.with(|context| {
            let context = context.borrow();
            let context = context.get(&self.1).unwrap();
            Arc::clone(&context.context)
        })
    }
}

pub fn hydrate_partial<E>(partial_id: String, element: Element<HtmlNode, E>)
where
    E: Send + 'static + Debug,
{
    spawn_local(async {
        let first_node = if let Some(first_node) = window()
            .expect("Failed to get Window")
            .document()
            .expect("Failed to get Document")
            .query_selector(&format!("[data-bloom-partial='{}']", partial_id))
            .expect("Failed to query selector for partial")
        {
            first_node
        } else {
            console::warn_2(&"Failed to find Partial Element".into(), &partial_id.into());
            return;
        };

        let root_dom_node = first_node
            .parent_element()
            .expect("Failed to get Parent for Partial Hydration");

        let root: Arc<HtmlNode> = Arc::new(
            HtmlNode::element(interned(root_dom_node.tag_name().to_lowercase()))
                .build()
                .into(),
        );
        let start_index = Array::from(&root_dom_node.child_nodes()).index_of(&first_node, 0);
        let context_id = u64::from_str_radix(
            &first_node
                .get_attribute("data-bloom-ctx")
                .expect("Failed to get attribute"),
            16,
        )
        .expect("Failed to parse context id");

        let dom =
            PartialDom::hydrate_from(context_id, root.clone(), root_dom_node.into(), start_index);

        if let Err(error) = render_loop(root, element, WasmSpawner, dom).await {
            let msg = format!("Render loop error: {:?}", error);
            console::error_1(&msg.into());
        }
    })
}
