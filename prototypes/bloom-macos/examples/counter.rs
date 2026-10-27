//! `cargo run --example counter` on a Mac.
#[cfg(target_os = "macos")]
fn main() {
    use std::sync::Arc;

    use async_trait::async_trait;
    use bloom_core::{use_state, Component, Element};
    use bloom_macos::node::{prelude::*, NativeNode};
    use bloom_rsx::rsx;

    #[derive(PartialEq)]
    struct Counter;

    #[async_trait]
    impl Component for Counter {
        type Node = NativeNode;
        type Error = ();

        async fn render(self: Arc<Self>) -> Result<Element<NativeNode, ()>, ()> {
            let count = use_state(|| 0);
            Ok(rsx!(
                <stack direction="column" gap=8 padding=16>
                    {format!("Count: {}", *count)}
                    <button title="Increment" on_click={move |_| count.update(|c| *c + 1)} />
                </stack>
            ))
        }
    }

    bloom_macos::appkit::run_app("bloom", Element::from(Counter));
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this example only runs on macOS");
}
