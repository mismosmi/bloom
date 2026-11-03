# bloom-server-example

An example of rendering bloom components on the server with [bloom-ssr](../bloom-ssr/README.md) and [axum](https://crates.io/crates/axum), and of hydrating them in the browser with [bloom-client](../bloom-client/README.md).

## Pages

* `/` (`?name=...`) is rendered on the server only.
* `/hydrate` is rendered on the server and fully hydrated on the client.
* `/hydrate-partial` is rendered on the server and only the interactive islands are hydrated on the client.

## Running it

Install the WebAssembly target once:

```sh
rustup target add wasm32-unknown-unknown
```

Then run the server from the workspace root:

```sh
cargo run -p bloom-server-example
```

Open http://localhost:3000.

The build script uses [wasm-pack](https://rustwasm.github.io/wasm-pack/) from your PATH,
or installs it with `cargo install wasm-pack --locked` into Cargo's build output directory
if it is missing. That first installation needs network access.

It compiles the client library to WebAssembly and generates its JavaScript
bindings before Cargo compiles the server. The server embeds these assets from Cargo's
build output directory. Changes to workspace sources rebuild the client bundle automatically.
Use `cargo run -p bloom-server-example --release` for optimized client and server builds.
