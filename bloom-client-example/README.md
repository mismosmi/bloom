# bloom-client-example

An example single page application that runs completely in the browser on WebAssembly using [bloom-client](../bloom-client/README.md).

## Running it

Build the WebAssembly bundle into `pkg/` with [wasm-pack](https://rustwasm.github.io/wasm-pack/):

```sh
wasm-pack build --target web
```

Then serve this directory with any static file server, for example:

```sh
python3 -m http.server
```

and open http://localhost:8000.
