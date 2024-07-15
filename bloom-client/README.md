# bloom-client

A client-side UI library similar to react-dom based on [bloom-core](https://crates.io/crates/bloom-core) and [bloom-html](https://crates.io/crates/bloom-html).

For server-side prerendering see [bloom-ssr](https://crates.io/crates/bloom-ssr)

## Setup
Docs on how to use this with wasm-pack are coming soon

## API
There's really only two APIs
* `render` renders a component completely on the client
* `hydrate` renders a component based on pre-rendered html (usually from using `bloom-ssr` on the server)

## Example
See [bloom-client-example](https://github.com/mismosmi/bloom/tree/main/bloom-client-example)