use std::{fmt::Debug, sync::Arc};

use bloom_core::{render_loop, Element};
use bloom_html::HtmlNode;
use dom::Dom;
use interned_str::interned;
use spawner::WasmSpawner;
use wasm_bindgen_futures::spawn_local;
use web_sys::{console, wasm_bindgen::JsCast, window, HtmlElement};

mod dom;
mod interned_str;
mod partial;
mod spawner;

pub use partial::hydrate_partial;

pub fn get_element_by_id(id: &str) -> Option<HtmlElement> {
    window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|element| element.dyn_into::<HtmlElement>().ok())
}

/// Use the render-function to construct the DOM for a component completely on the client.
/// Pass it your bloom-component and the root HtmlElement to render it into.
/// ```
/// use bloom_client::{render, get_element_by_id};
/// use bloom_rsx::rsx;
///
/// #[wasm_bindgen(start)]
/// fn run() {
///     render(get_element_by_id("root").unwrap(), rsx!(<MyComponent />));
/// }
/// ```
pub fn render<E>(root: HtmlElement, element: Element<HtmlNode, E>)
where
    E: Send + 'static + Debug,
{
    spawn_local(async {
        let mut dom = Dom::new();

        let root_node = Arc::new(
            HtmlNode::element(interned(root.tag_name().to_lowercase()))
                .build()
                .into(),
        );
        dom.register(&root_node, root.into());
        if let Err(error) = render_loop(root_node, element, WasmSpawner, dom).await {
            let msg = format!("Render loop error: {:?}", error);
            console::error_1(&msg.into());
        }
    });
}

/// hydrate can be used to hydrate an existing DOM from server-side rendered HTML.
pub fn hydrate<E>(root: HtmlElement, element: Element<HtmlNode, E>)
where
    E: Send + 'static + Debug,
{
    spawn_local(async {
        let mut dom = Dom::hydrate();

        let root_node = Arc::new(
            HtmlNode::element(interned(root.tag_name().to_lowercase()))
                .build()
                .into(),
        );
        dom.register(&root_node, root.into());
        if let Err(error) = render_loop(root_node, element, WasmSpawner, dom).await {
            let msg = format!("Render loop error: {:?}", error);
            console::error_1(&msg.into());
        }
    });
}
