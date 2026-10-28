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
* `Suspense`
* `ErrorBoundary`

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

Wrap components in an `ErrorBoundary` to catch their errors. When one of the children fails, all of them are removed
and the fallback, built from the error, is shown in their place:
```rust
rsx!(
    <ErrorBoundary fallback={|error: CaughtError<MyError>| rsx!(
        <div>{error.to_string()}</div>
        <button on_click={move |_| error.dismiss()}>"Try again"</button>
    )}>
        <Profile />
    </ErrorBoundary>
)
```
The fallback gets a `CaughtError`, which dereferences to the error and is cheap to clone.
The fallback stays until the error is dismissed with `dismiss()`, which renders the component around the
boundary again and creates the children from scratch, or until the boundary is created again, e.g. by changing its key.
Errors of the fallback itself, and errors outside of any boundary, are passed on to the next boundary above
or end the render loop.

## Keyed lists
Children are matched to the previous render by their position.
When a list is reordered, that hands the state of one item to another and rewrites every node in place.
Keys identify children instead, so they keep their state and native nodes and are only moved.

`keyed(key, element)` wraps any element in a keyed element, and `rsx!` calls it for every tag or
component with a `key`, e.g. `<li key={id}>` or `<Row key={id} />`. To key several elements at once,
wrap them in a `<Fragment key={id}>` (`bloom_core::Fragment`).

Any hashable value can be a key. Only its 64 bit hash (`Key`) is stored, so two different keys colliding is possible but negligibly unlikely.
Keys only have to be unique among their siblings. As soon as one child has a key the whole list is matched by key,
children without a key are then matched by their position among the other unkeyed children.
A changed key always replaces the element, even outside of lists, which rebuilds components with fresh state.

Renderers implement `ObjectModel::move_node` to move an existing node.

## Render passes
Every state update triggers a render pass. Each component that renders is rendered and diffed in its own task on the spawner passed to the render loop,
so with a multi-threaded spawner sibling components render in parallel. While rendering, changes to the native nodes are only collected.
Once all components are done, the pass is committed synchronously in one go: the cleanups run first,
then the native nodes are updated and finally the effects run. A cleanup therefore still sees the nodes of its component,
and an effect sees the nodes of the whole render pass.

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

## Suspense
Wrap async components in a `Suspense` boundary to show a fallback until all of them have rendered:
```rust
rsx!(
    <Suspense fallback="Loading...">
        <Profile />
        <Posts />
    </Suspense>
)
```
The children are shown together once all of them are done, and their effects only run at that point.
The fallback is only shown on the first render: when a child suspends again later on, its previous output stays on screen until the new one is ready.

Server-side rendering ignores suspense boundaries for now and always waits for the children.

## HTML
For everything related to rendering HTML with bloom see the [bloom-html](https://crates.io/crates/bloom-html) crate.