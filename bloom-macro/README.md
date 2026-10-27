# bloom-macro

Procedural macros for the bloom ecosystem. Use them through [bloom-core](https://crates.io/crates/bloom-core), which re-exports them.

## `#[component]`

Turns a function into a component. The function arguments become the props of the component:

```rust
use bloom_core::{component, use_state, Element};
use bloom_html::{prelude::*, HtmlNode};
use bloom_rsx::rsx;

#[component]
fn Counter(label: String) -> Result<Element<HtmlNode, ()>, ()> {
    let count = use_state(|| 0);
    Ok(rsx!(
        <div>{format!("{}: {}", label, *count)}</div>
        <button on_click=move |_| count.update(|count| *count + 1)>"Increment"</button>
    ))
}

#[component]
fn App() -> Result<Element<HtmlNode, ()>, ()> {
    Ok(rsx!(<Counter label="Clicks" />))
}
```

The macro generates the props struct, a builder that accepts anything that converts into the prop types and the `Component` implementation.
Leaving out a prop is a compile-time error.
