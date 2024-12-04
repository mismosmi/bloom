#![cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use axum::{async_trait, body::Body, extract::Query, routing::get, Router};
use bloom_core::{Component, Element};
use bloom_html::{tag::div, text, HtmlNode};
use bloom_server_example::{hydration, partial_hydration, TokioSpawner};
use bloom_ssr::render_to_stream;
use builder_pattern::Builder;
use serde::Deserialize;

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route("/", get(home))
        .route("/hydrate", get(hydration::server::hydration_page))
        .route("/hydrate-partial", get(partial_hydration::hydrate()))
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

#[derive(PartialEq, Builder)]
struct HomePage {
    name: String,
}

#[async_trait]
impl Component for HomePage {
    type Error = anyhow::Error;
    type Node = HtmlNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        Ok(div()
            .build()
            .children(vec![text(format!("Hello, {}!", self.name))]))
    }
}

async fn home(query: Query<QueryParams>) -> Body {
    Body::from_stream(render_to_stream(
        HomePage::new()
            .name(query.name.clone().unwrap_or("World".to_string()))
            .build()
            .into(),
        TokioSpawner,
    ))
}
