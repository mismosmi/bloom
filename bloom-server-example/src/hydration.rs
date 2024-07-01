use std::sync::Arc;

use async_trait::async_trait;
use bloom_client::get_element_by_id;
use bloom_core::{use_state, Component, Element};
use bloom_html::{prelude::*, HtmlNode};
use bloom_rsx::{rsx, NoopBuilder};
use builder_pattern::Builder;

#[derive(PartialEq, NoopBuilder)]
struct HydrationPage;

#[async_trait]
impl Component for HydrationPage {
    type Error = anyhow::Error;
    type Node = HtmlNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        let counter = use_state::<u32>();
        Ok(rsx!(
            <div>
                "Hello, World!"
                <div>
                    {counter.to_string()}
                    <button on_click=move |_| counter.update(|count| *count + 1)>
                        "Increase"
                    </button>
                </div>
                <script type="module">
                    "import init, { hydrate } from \"/bundle.js\"; await init(); await hydrate();"
                </script>
            </div>
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod server {
    use super::*;
    use bloom_rsx::rsx;
    use bloom_server::render_to_stream;

    pub async fn hydration_page() -> axum::body::Body {
        use axum::body::Body;

        use crate::TokioSpawner;

        Body::from_stream(render_to_stream(
            rsx!(
                <div id="root">
                    <HydrationPage />
                </div>
            ),
            TokioSpawner,
        ))
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    bloom_client::hydrate(get_element_by_id("root").unwrap(), rsx!(<HydrationPage />));
}
