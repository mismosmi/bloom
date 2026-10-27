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
/// ```no_run
/// # use std::sync::Arc;
/// # use async_trait::async_trait;
/// # use bloom_core::{Component, Element};
/// # use bloom_html::{prelude::*, HtmlNode};
/// use bloom_client::{get_element_by_id, render};
/// use bloom_rsx::{rsx, NoopBuilder};
/// use wasm_bindgen::prelude::*;
///
/// # #[derive(PartialEq, NoopBuilder)]
/// # struct MyComponent;
/// # #[async_trait]
/// # impl Component for MyComponent {
/// #     type Node = HtmlNode;
/// #     type Error = ();
/// #     async fn render(self: Arc<Self>) -> Result<Element<HtmlNode, ()>, ()> {
/// #         Ok(rsx!(<div />))
/// #     }
/// # }
/// #[wasm_bindgen(start)]
/// fn run() {
///     render(get_element_by_id("root").unwrap(), rsx!(<MyComponent />));
/// }
/// # fn main() {}
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

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests {
    use std::{cell::RefCell, sync::Arc};

    use async_trait::async_trait;
    use bloom_core::{use_state, Component, Element};
    use bloom_html::{prelude::*, HtmlNode};
    use bloom_rsx::rsx;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{js_sys::Promise, wasm_bindgen::JsCast, window, HtmlElement};

    use super::render;

    wasm_bindgen_test_configure!(run_in_browser);

    async fn next_frame() {
        let frame = Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        });
        JsFuture::from(frame).await.unwrap();
    }

    #[wasm_bindgen_test]
    async fn render_root_node() {
        let root: HtmlElement = window()
            .unwrap()
            .document()
            .unwrap()
            .create_element("div")
            .unwrap()
            .dyn_into()
            .unwrap();
        let element: Element<HtmlNode, ()> = rsx!(<span id="a">"hi"</span>"!");

        render(root.clone(), element);
        next_frame().await;
        next_frame().await;

        assert_eq!(root.inner_html(), "<span id=\"a\">hi</span>!");
    }

    thread_local! {
        static REVERSE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
    }

    #[derive(PartialEq)]
    struct ReversibleList;

    #[async_trait]
    impl Component for ReversibleList {
        type Node = HtmlNode;
        type Error = ();

        async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
            let reversed = use_state(|| false);
            let reverse = reversed.clone();
            REVERSE.with(|cell| {
                *cell.borrow_mut() = Some(Box::new(move || reverse.update(|_| true)));
            });
            let ids = if *reversed {
                vec![3, 2, 1]
            } else {
                vec![1, 2, 3]
            };
            Ok(
                rsx!(<ul>{ids.into_iter().map(|id| rsx!(<li key={id}>{id.to_string()}</li>)).collect::<Vec<_>>()}</ul>),
            )
        }
    }

    #[wasm_bindgen_test]
    async fn reorder_keyed_list() {
        let root: HtmlElement = window()
            .unwrap()
            .document()
            .unwrap()
            .create_element("div")
            .unwrap()
            .dyn_into()
            .unwrap();

        render(root.clone(), ReversibleList.into());
        next_frame().await;
        next_frame().await;
        assert_eq!(root.inner_html(), "<ul><li>1</li><li>2</li><li>3</li></ul>");
        let list = root.first_child().unwrap();
        let first_item = list.first_child().unwrap();

        REVERSE.with(|cell| cell.borrow().as_ref().unwrap()());
        next_frame().await;
        next_frame().await;
        assert_eq!(root.inner_html(), "<ul><li>3</li><li>2</li><li>1</li></ul>");
        assert!(
            list.last_child().unwrap().is_same_node(Some(&first_item)),
            "The list item should be moved, not recreated"
        );
    }
}
