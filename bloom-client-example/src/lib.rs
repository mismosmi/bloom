use std::{panic, sync::Arc};

use async_trait::async_trait;
use bloom_client::{get_element_by_id, render};
use bloom_core::{use_effect, use_ref, use_state, Component, Element};
use bloom_html::{tag, tag::div, DomRef, HtmlNode};
use bloom_rsx::rsx;
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
        let hello_world_ref: Arc<DomRef> = use_ref();

        use_effect(hello_world_ref.clone(), |node| {
            console::log_2(&"hello world div".into(), &node.get().unwrap());
        });

        let counter = use_state::<i32>();
        Ok(rsx!(
            <div>
                <div ref=hello_world_ref>
                    "Hello, World!"
                </div>
                <div>{counter.to_string()}</div>
                <button on_click=move |_| counter.update(|count| *count + 1)>
                    "Increase"
                </button>
            </div>
        ))
    }
}
