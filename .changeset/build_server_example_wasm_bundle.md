---
bloom: patch
---

# Build the server example's client bundle automatically

Generate and embed the client Wasm and JavaScript assets when building the server
example. Install `wasm-pack` into the build output directory if it is unavailable,
and isolate nested builds to avoid recursion and Cargo lock contention.
