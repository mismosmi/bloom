//! `cargo run --example counter` on a Mac.
#[cfg(target_os = "macos")]
fn main() {
    use std::sync::Arc;

    use async_trait::async_trait;
    use bloom_appkit::{AppKitNode, Button, StackView, TextField, Window};
    use bloom_core::{use_state, Component, Element};
    use bloom_rsx::rsx;
    use objc2_app_kit::{NSLayoutAttribute, NSUserInterfaceLayoutOrientation};
    use objc2_foundation::{NSEdgeInsets, NSSize};

    #[derive(PartialEq)]
    struct Counter;

    #[async_trait]
    impl Component for Counter {
        type Node = AppKitNode;
        type Error = ();

        async fn render(self: Arc<Self>) -> Result<Element<AppKitNode, ()>, ()> {
            let count = use_state(|| 0);
            let name = use_state(String::new);
            let greeting = format!("Hello, {}! Count: {}", *name, *count);
            Ok(rsx!(
                <Window title="bloom" content_size={NSSize::new(320.0, 160.0)}>
                    <StackView
                        orientation={NSUserInterfaceLayoutOrientation::Vertical}
                        alignment={NSLayoutAttribute::Leading}
                        spacing=8
                        edge_insets={NSEdgeInsets { top: 16.0, left: 16.0, bottom: 16.0, right: 16.0 }}
                    >
                        {greeting}
                        <TextField
                            placeholder_string="Your name"
                            string_value={name.to_string()}
                            on_change={move |field| {
                                // read on the main thread; only plain data goes into state
                                let value = field.stringValue().to_string();
                                name.update(move |_| value)
                            }}
                        />
                        <Button title="Increment" on_action={move |_| count.update(|c| *c + 1)} />
                    </StackView>
                </Window>
            ))
        }
    }

    bloom_appkit::run(Element::from(Counter));
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this example only runs on macOS");
}
