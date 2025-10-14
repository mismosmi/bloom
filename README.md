# bloom

Bloom is react-like UI framework in rust.

This is a proof of concept for a react-like diffing rendering model with an abstract core allowing
for diverse rendering backends from HTML for browsers through native UI frameworks in order
to build true native cross platform GUI applications.

It is split into the following packages:

* [bloom-core] is the core package including the diffing logic and core traits.
* [bloom-html] provides node definitions for working with HTML rendering targets.
* [bloom-client] is a reference implementation for using bloom on web-clients using web assembly.
* [bloom-ssr] is a reference implementation for server-side rendering, both to static strings and as streaming responses.
* [bloom-rsx] provides the procedural `rsx!` macro used to define views with bloom.
* [bloom-macro] provides some additional macros to simplify defining components.
* [async-context] allows providing context to async rust functions allowing bloom-components (in contrast to react components) to be truly async.
* [bloom-client-example] is an example implementation of an SPA (runnning completely in the browser on web assembly) with bloom.
* [bloom-server-example] is an example implementation of using bloom to render html on the server.
