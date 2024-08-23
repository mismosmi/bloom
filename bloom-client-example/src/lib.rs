use std::{panic, sync::Arc};

use bloom_client::{get_element_by_id, render};
use bloom_core::{use_effect, use_ref, use_state};
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

#[bloom_core::component]
fn ExampleApp() -> Result<Element<HtmlNode, ()>, ()> {
    console::log_1(&"Rendering ExampleApp".into());
    console::log_1(&format!("{:?}", div().build()).into());
    let hello_world_ref: Arc<DomRef> = use_ref();

    use_effect(hello_world_ref.clone(), |node| {
        console::log_2(&"hello world div".into(), &node.get().unwrap());
    });

    let counter = use_state(|| 0i32);
    Ok(rsx!(
        <div>
            <div ref=hello_world_ref>
                "Hello, World!"
            </div>
            <div>{counter.to_string()}</div>
            <button on_click=move |_| counter.update(|count| *count + 1)>
                "Increase"
            </button>
            <MacroComponent label="Hello, Macro!" />
        </div>
    ))
}

#[bloom_core::component]
fn MacroComponent(label: String) -> Result<Element<HtmlNode, ()>, ()> {
    Ok(rsx!(
        <div>{label}</div>
    ))
}
