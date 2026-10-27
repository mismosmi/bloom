# bloom-server-example

An example of rendering bloom components on the server with [bloom-ssr](../bloom-ssr/README.md) and [axum](https://crates.io/crates/axum), and of hydrating them in the browser with [bloom-client](../bloom-client/README.md).

## Pages

* `/` (`?name=...`) is rendered on the server only.
* `/hydrate` is rendered on the server and fully hydrated on the client.
* `/hydrate-partial` is rendered on the server and only the interactive islands are hydrated on the client.

## Running it

The server embeds the WebAssembly bundle for the client, so build it into `pkg/` first with [wasm-pack](https://rustwasm.github.io/wasm-pack/):

```sh
wasm-pack build --target web
cargo run
```

and open http://localhost:3000.
