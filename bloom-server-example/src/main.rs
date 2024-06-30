use std::sync::Arc;

use axum::{async_trait, body::Body, extract::Query, routing::get, Router};
use bloom_core::{Component, Element};
use bloom_html::{tag::div, text, HtmlNode};
use bloom_server::render_to_stream;
use derive_builder::Builder;
use serde::Deserialize;

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new().route("/", get(home));

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
    type Error = HomePageBuilderError;
    type Node = HtmlNode;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        Ok(div().children(vec![text(format!("Hello, {}!", self.name))]))
    }
}

#[derive(Clone)]
struct TokioSpawner;
impl futures_util::task::Spawn for TokioSpawner {
    fn spawn_obj(
        &self,
        future: futures_util::task::FutureObj<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        tokio::spawn(future);
        Ok(())
    }
}

async fn home(query: Query<QueryParams>) -> Body {
    Body::from_stream(render_to_stream(
        HomePageBuilder::default()
            .name(query.name.clone().unwrap_or("World".to_string()))
            .build()
            .expect("Failed to build home page")
            .into(),
        TokioSpawner,
    ))
}
