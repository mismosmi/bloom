# bloom-macro

Procedural macros for the bloom ecosystem. Use them through [bloom-core](https://crates.io/crates/bloom-core), which re-exports them.

## `#[derive(Identify)]`

Combines field identities for structs and enums used as hook arguments. Enums
also include the variant discriminant. Supports named, tuple, and unit forms,
with generics and existing where clauses; every field must implement `Identify`.

```rust
use bloom_core::Identify;

#[derive(Identify)]
struct Inputs {
    label: String,
    count: usize,
}

assert_eq!(
    Inputs { label: "hello".into(), count: 2 }.id(),
    Inputs { label: "hello".into(), count: 2 }.id(),
);
```

All `Hash` types already implement `Identify`, so derive `Hash` or `Identify`,
but not both. Deriving `Identify` is useful when fields have custom `Identify`
implementations rather than implementing `Hash`.

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

The macro generates the props struct, a [bon](https://docs.rs/bon) builder that accepts anything that converts into the prop types and the `Component` implementation.
Props of type `Option<T>` are optional, leaving out any other prop is a compile-time error.
Crates using the macro don't need to depend on `bon` themselves.
