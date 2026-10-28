# bloom-appkit (prototype)

Throwaway spike backing [docs/macos-renderer.md](../../docs/macos-renderer.md).
It is not part of the workspace and is not published.

The nodes mirror AppKit classes and their properties: there are no string tags and no CSS.

```rust
rsx!(
    <Window title="bloom">
        <StackView orientation={NSUserInterfaceLayoutOrientation::Vertical} spacing=8>
            "Hello"
            <Button title="OK" on_action={|sender: &NSButton| ...} />
        </StackView>
    </Window>
)
```

* `src/node.rs` – `Window`, `StackView`, `Button`, `TextField`, plus `Outlet<T>` (the `use_ref` equivalent of an IBOutlet)
* `src/appkit.rs` – the `ObjectModel` over AppKit via [objc2](https://docs.rs/objc2): target/action, the text field delegate, and stack-view arranged subviews
* `src/executor.rs` – runs the render loop on the main GCD queue
* `tests/headless.rs` – runs `render_loop` with a recording `ObjectModel`; it creates no AppKit objects

Everything is macOS only.

```sh
cargo clippy --all-targets --target aarch64-apple-darwin  # type-check from any OS
cargo test                                                # on a Mac
cargo run --example counter                               # on a Mac
```
