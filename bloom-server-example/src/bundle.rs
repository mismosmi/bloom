#![cfg(not(target_arch = "wasm32"))]

use axum::response::IntoResponse;

static BUNDLE_JS: &[u8] = include_bytes!("../pkg/bloom_server_example.js");
static BUNDLE_WASM: &[u8] = include_bytes!("../pkg/bloom_server_example_bg.wasm");

pub async fn bundle_js() -> impl IntoResponse {
    ([("Content-Type", "text/javascript")], BUNDLE_JS)
}

pub async fn bundle_wasm() -> impl IntoResponse {
    ([("Content-Type", "application/wasm")], BUNDLE_WASM)
}

