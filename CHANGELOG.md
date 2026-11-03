## 0.2.2 (2026-11-03)

### Features

#### Add use_spawn hook

Expose the renderer's spawner to components through `use_spawn` and a cloneable,
type-erased `Spawner`. Keep renderer entry points generic while using `Spawner`
internally. Stream-rendering entry points now also require spawners to be `Sync`.

#### Add custom hook identities and memoized callbacks

Expose the `Identify` trait and derive macro for structs and enums, allowing
custom identities for `use_memo` and `use_effect` arguments. Existing `Hash`
types continue to work through a blanket implementation.

Add `use_callback` to memoize an owned callback in an `Arc`, refreshing it when
the captures' identity or callback function changes.

### Fixes

- Migrate all crates to the Rust 2024 edition and update the workspace to Cargo resolver 3.

#### Build the server example's client bundle automatically

Generate and embed the client Wasm and JavaScript assets when building the server
example. Install `wasm-pack` into the build output directory if it is unavailable,
and isolate nested builds to avoid recursion and Cargo lock contention.

## 0.2.1 (2026-11-02)

### Features

- Add use_memo hook

### Fixes

- Publish the Bloom libraries and async-context through the coordinated release workflow.
