use std::{panic, sync::Arc};

use async_trait::async_trait;
use bloom_client::{get_element_by_id, render};
use bloom_core::{use_state, Component, Element};
use bloom_html::{
    tag::{button, div},
    text, HtmlNode,
};
use wasm_bindgen::prelude::*;
use web_sys::console;

#[wasm_bindgen(start)]
fn main() -> Result<(), JsValue> {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    console::log_1(&"Hello, world!".into());
    console::log_1(&get_element_by_id("bloomroot").unwrap().tag_name().into());

    render(
        get_element_by_id("bloomroot").expect("Root node not found"),
        ExampleApp.into(),
    );

    Ok(())
}

#[derive(PartialEq)]
struct ExampleApp;

#[async_trait]
impl Component for ExampleApp {
    type Node = HtmlNode;
    type Error = ();

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        console::log_1(&"Rendering ExampleApp".into());
        console::log_1(&format!("{:?}", div().build()).into());

        let counter = use_state::<i32>();
        Ok(div().children(vec![
            div().children(vec![text("Hello, World!")]),
            div().children(vec![text(counter.to_string())]),
            button()
                .on("click", move |_| {
                    counter.update(|count| Arc::new(*count + 1))
                })
                .children(vec![text("Increase")]),
        ]))
    }
}
