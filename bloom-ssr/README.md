# bloom-ssr

Server-Side Rendering for the bloom-framework.

See [bloom-core](https://crates.io/crates/bloom-core) for the basics or [bloom-html](https://crates.io/crates/bloom-html) for how to use bloom in a browser context.

See [bloom-client](https://crates.io/crates/bloom-client) for how to add client-side rendering.

## Basic API
bloom-ssr provides two basic APIs: `render_to_stream` and `render_to_string` to render bloom components for the web (`Component<Node = bloom_html::HtmlNode>`) in a server-side setting.

## Web framework support
Tested with `axum`.
See [bloom-server-example](https://github.com/mismosmi/bloom/tree/main/bloom-server-example) for an example implementation.