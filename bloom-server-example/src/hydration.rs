use std::sync::Arc;

use async_trait::async_trait;
use bloom_client::get_element_by_id;
use bloom_core::{use_state, Component, Element};
use bloom_html::{
    tag::{button, div, script},
    text, HtmlNode,
};

#[derive(PartialEq)]
struct HydrationPage;

#[async_trait]
impl Component for HydrationPage {
    type Error = anyhow::Error;
    type Node = HtmlNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        let counter = use_state::<u32>();
        Ok(div().build().children(vec![
            text("Hello, World!"),
            div().build().children(vec![
                text(*counter),
                button()
                    .on("click", move |_| counter.update(|count| *count + 1))
                    .build()
                    .children(vec![text("Increase")]),
            ]),
            script().attr("type", "module").build().children(vec![text(
                "import init, { hydrate } from \"/bundle.js\"; await init(); await hydrate();",
            )]),
        ]))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod server {
    use bloom_html::tag::div;
    use bloom_server::render_to_stream;

    pub async fn hydration_page() -> axum::body::Body {
        use axum::body::Body;

        use crate::TokioSpawner;

        Body::from_stream(render_to_stream(
            div()
                .attr("id", "root")
                .build()
                .children(vec![super::HydrationPage.into()]),
            TokioSpawner,
        ))
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    bloom_client::hydrate(get_element_by_id("root").unwrap(), HydrationPage.into());
}
