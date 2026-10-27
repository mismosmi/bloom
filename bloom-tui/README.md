# bloom-tui

Terminal UI rendering for the bloom-framework.

See [bloom-core](https://crates.io/crates/bloom-core) for the basics.

bloom-tui implements bloom's `ObjectModel` for terminals:
* the node tree is laid out with flexbox by [taffy](https://crates.io/crates/taffy)
* nodes are painted into a [ratatui](https://ratatui.rs) cell buffer, ratatui only writes the cells that changed since the last frame
* every frame is wrapped in synchronized output, so the terminal never shows half-drawn frames
* text is wrapped by grapheme and display width, so emoji and CJK text line up
* input comes from [crossterm](https://crates.io/crates/crossterm), with mouse support, bracketed paste and the kitty keyboard protocol where available

## Example

```rust
use bloom_core::{component, use_state, Element};
use bloom_rsx::rsx;
use bloom_tui::{prelude::*, render, TuiNode};

#[component]
fn Counter() -> Result<Element<TuiNode, ()>, ()> {
    let count = use_state(|| 0);
    let increment = count.clone();

    Ok(rsx!(
        <column border="rounded" title="Counter" padding="0 1">
            <text>"Count: " <span bold color="green">{count.to_string()}</span></text>
            <box on_click={move |_| increment.update(|count| *count + 1)}>
                <text reversed>" Click me "</text>
            </box>
        </column>
    ))
}

fn main() {
    let spawner = futures_executor::ThreadPool::new().unwrap();
    futures_executor::block_on(render(rsx!(<Counter />), spawner)).unwrap();
}
```

Run the full example with `cargo run -p bloom-tui --example counter`.

## Elements and attributes

* `<box>` / `<row>` lay out their children in a row, `<column>` / `<col>` in a column.
* `<text>` / `<span>` / `<p>` are paragraphs, nested elements inside them only contribute their text style.
* Layout attributes follow CSS flexbox: `grow`, `shrink`, `width`, `height`, `padding="1 2"`, `margin`, `gap`, `justify`, `align`, `position="absolute"`, `overflow="hidden"`, ...
* Appearance: `border` (`plain`, `rounded`, `double`, `thick`), `border_color`, `title`, `color`, `bg`, `bold`, `dim`, `italic`, `underline`, `reversed`, `strikethrough`.
* Events: `on_key`, `on_paste`, `on_resize`, `on_focus` are delivered to every element defining them, `on_click`, `on_scroll`, `on_mouse` to the topmost element under the mouse.

See the crate documentation for the full list.

## Testing

`TestTerminal` renders components into an in-memory buffer and lets you send events to them,
see the [tests](./tests/render.rs) for examples.

## Async runtimes

`render` takes any `futures` spawner to run async components.
Enable the `tokio` feature to use `bloom_tui::TokioSpawner`.
