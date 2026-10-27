# bloom-macos (prototype)

Throwaway spike backing [docs/macos-renderer.md](../../docs/macos-renderer.md).
Not part of the workspace and not published.

* `src/node.rs` – platform-neutral `NativeNode` with an rsx-compatible `tag()` builder
* `src/layout.rs` – flexbox layout through [taffy](https://docs.rs/taffy)
* `src/executor.rs` – runs the render loop on the main GCD queue (macOS only)
* `src/appkit.rs` – `ObjectModel` over AppKit via [objc2](https://docs.rs/objc2) (macOS only)
* `tests/headless.rs` – drives `render_loop` with `NativeNode` on any OS

```sh
cargo test                                            # any OS
cargo clippy --all-targets --target aarch64-apple-darwin  # type-check AppKit code from Linux
cargo run --example counter                           # on a Mac
```
