#![cfg(not(target_arch = "wasm32"))]
use axum::{Router, body::Body, extract::Query, routing::get};
use bloom_core::{Element, component};
use bloom_html::{HtmlNode, tag::div, text};
use bloom_server_example::{TokioSpawner, hydration, partial_hydration};
use bloom_ssr::render_to_stream;
use serde::Deserialize;

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route("/", get(home))
        .route("/hydrate", get(hydration::server::hydration_page))
        .route(
            "/hydrate-partial",
            get(partial_hydration::server::hydration_page),
        )
        .route("/bundle.js", get(bloom_server_example::bundle::bundle_js))
        .route(
            "/bloom_server_example_bg.wasm",
            get(bloom_server_example::bundle::bundle_wasm),
        );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Deserialize)]
struct QueryParams {
    name: Option<String>,
}

#[component]
fn HomePage(name: String) -> Result<Element<HtmlNode, anyhow::Error>, anyhow::Error> {
    Ok(div()
        .build()
        .children(vec![text(format!("Hello, {}!", name))]))
}

async fn home(query: Query<QueryParams>) -> Body {
    Body::from_stream(render_to_stream(
        HomePage::builder()
            .name(query.name.clone().unwrap_or("World".to_string()))
            .build()
            .into(),
        TokioSpawner,
    ))
}
