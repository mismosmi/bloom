# bloom-core

Bloom is a descriptive UI framework closely modeled after react.

## Core APIs
A UI in bloom is made up of Components (structs that implement the `Component` trait).

It provides APIs similar to those of react:
* `use_state`
* `use_ref`
* `use_effect`
* `use_context` (WIP)
* `use_layout_effect` (WIP)
* `Suspense` (WIP)
* `ErrorBoundary` (WIP)

that can be used in the `render`-method of the `Component`-trait for features such as state, side-effects or raw references to the underlying renderer.

The core crate also provides a default render-loop implementation which makes it easy to implement additional renderers.

## Why not reactive
Most modern UI frameworks use an architecture based on reactive-programming primitives such as signals.

React, on the other hand, just re-renders the entire component tree (or parts of it) whenever some state changes which guarantees that the UI is always updated to match the exact output of the respective render functions.

Practical Experience has shown that reactive programming is hard and, while generally enabling better performance as it avoids the diffing step (figuring out which actual properties of the UI have changed) they also introduce performance pitfalls and hard-to-debug reactivity bugs.

Bloom is an attempt to bring the positive sides of react to rust UI development.

## Why not the elm-architecture
A lot of Rust Web-Frameworks are based on the elm-architecture (where you provide a model and an update-function that takes the previous model and an Event-object and builds the next state for the next iteration of the UI).

This approach is very clean in a functional sense but introduces boilerplate and is hard to scale as it makes it hard to create isolated components that work completely on their own.


## Error handling
The `Component`-trait has an associated type `Error` that represents the type of Error that the render function might return.

This enables the consumer of the library to use their own error type, which might be `anyhow::Error`, an error type generated with `thiserror` or a completely custom error type.

A reasonable API to catch these errors (`ErrorBoundary`) is in the making.

## Renderer-Agnostic
While react is build mainly for the web (react-native being an afterthought) bloom is generally renderer agnostic.

Native UI elements (such as native HTML-Nodes, think `<div>` and `<span>`) are called "Nodes" in the bloom-world. Besides HTML Elements, bloom could also be used to render QT-Objects such as `Button`s or basically any other UI primitive.

The `Component`-trait has a type parameter (`Node`) to represent the specific node-type it is implemented for.

## RSX
Bloom provides its own implementation of a JSX-like syntax for rust, RSX, in the [bloom-rsx](https://crates.io/crates/bloom-rsx) crate.

## Data fetching
Bloom components are async functions. This means that data fetching can be as easy as a direct fetch-call:
```rust
async fn render() -> Result<Element<Self::Node, Self::Error>, Self::Error> {
    let my_data = reqwest::get(my_api_endpoint).await?.json().await?;

    rsx!(
        <Heading>{my_data.title}</Heading>
    )
}
```

## HTML
For everything related to rendering HTML with bloom see the [bloom-html](https://crates.io/crates/bloom-html) crate.