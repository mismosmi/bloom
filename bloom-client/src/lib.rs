use std::{fmt::Debug, sync::Arc};

use bloom_core::{render_loop, Element};
use bloom_html::HtmlNode;
use dom::Dom;
use spawner::WasmSpawner;
use wasm_bindgen_futures::spawn_local;
use web_sys::{console, wasm_bindgen::JsCast, window, HtmlElement};

mod dom;
mod spawner;

pub fn get_element_by_id(id: &str) -> Option<HtmlElement> {
    window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|element| element.dyn_into::<HtmlElement>().ok())
}

pub fn render<E>(root: HtmlElement, element: Element<HtmlNode, E>)
where
    E: Send + 'static + Debug,
{
    spawn_local(async {
        let mut dom = Dom::new();

        let root_node = Arc::new(HtmlNode::element(root.tag_name()).into());
        dom.register(&root_node, &root.into());
        if let Err(error) = render_loop(root_node, element, WasmSpawner, dom).await {
            let msg = format!("Render loop error: {:?}", error);
            console::error_1(&msg.into());
        }
    });
}

pub fn hydrate<E>(root: HtmlElement, element: Element<HtmlNode, E>)
where
    E: Send + 'static + Debug,
{
    spawn_local(async {
        let mut dom = Dom::hydrate();

        let root_node = Arc::new(HtmlNode::element(root.tag_name()).into());
        dom.register(&root_node, &root.into());
        if let Err(error) = render_loop(root_node, element, WasmSpawner, dom).await {
            let msg = format!("Render loop error: {:?}", error);
            console::error_1(&msg.into());
        }
    });
}
