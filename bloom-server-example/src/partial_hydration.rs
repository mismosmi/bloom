//! Partial hydration: the page is rendered on the server and only the
//! interactive "islands" are hydrated on the client. Everything else stays
//! static HTML and never runs on the client.
use bloom_core::{component, use_state, Element};
use bloom_html::{prelude::*, HtmlNode};
use bloom_rsx::rsx;

/// An interactive island.
/// `bloom_client::hydrate_partial` finds the island by its `data-bloom-partial`
/// attribute and starts hydrating at that element, so it has to be the root
/// node of the island. `data-bloom-ctx` is a hex id for the island's context.
#[component]
fn Island(
    name: String,
    context_id: String,
) -> Result<Element<HtmlNode, anyhow::Error>, anyhow::Error> {
    let counter = use_state(|| 0i32);
    Ok(rsx!(
        <div data-bloom-partial={name.clone()} data-bloom-ctx={context_id.clone()}>
            {format!("{}: {}", name, *counter)}
            <button on_click=move |_| counter.update(|count| *count + 1)>
                "Increase"
            </button>
        </div>
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub mod server {
    use super::*;
    use bloom_ssr::render_to_stream;

    use crate::TokioSpawner;

    pub async fn hydration_page() -> axum::body::Body {
        axum::body::Body::from_stream(render_to_stream(
            rsx!(
                <div>
                    <h1>"Partial Hydration"</h1>
                    <p>"This text is only rendered on the server."</p>
                    <Island name="first" context_id="1" />
                    <p>"Static content between the islands."</p>
                    <Island name="second" context_id="2" />
                    <script type="module">
                        "import init, { hydrate_islands } from \"/bundle.js\"; await init(); hydrate_islands();"
                    </script>
                </div>
            ),
            TokioSpawner,
        ))
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_islands() {
    bloom_client::hydrate_partial(
        "first".to_string(),
        rsx!(<Island name="first" context_id="1" />),
    );
    bloom_client::hydrate_partial(
        "second".to_string(),
        rsx!(<Island name="second" context_id="2" />),
    );
}
