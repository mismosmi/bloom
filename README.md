# bloom

Bloom is react-like UI framework in rust.

This is a proof of concept for a react-like diffing rendering model with an abstract core allowing
for diverse rendering backends from HTML for browsers through native UI frameworks in order
to build true native cross platform GUI applications.

It is split into the following packages:

* [bloom-core](./bloom-core/README.md) is the core package including the diffing logic and core traits.
* [bloom-html](./bloom-html/README.md) provides node definitions for working with HTML rendering targets.
* [bloom-client](./bloom-client/README.md) is a reference implementation for using bloom on web-clients using web assembly.
* [bloom-gtk](./bloom-gtk/README.md) renders bloom components to native GTK4 widgets.
* [bloom-ssr](./bloom-ssr/README.md) is a reference implementation for server-side rendering, both to static strings and as streaming responses.
* [bloom-rsx](./bloom-rsx/README.md) provides the procedural `rsx!` macro used to define views with bloom.
* [bloom-macro](./bloom-macro/README.md) provides some additional macros to simplify defining components.
* [async-context](./async-context/README.md) allows providing context to async rust functions building the foundation of bloom's runtime hooks and allowing bloom-components (in contrast to react components) to be truly async.
* [bloom-client-example](./bloom-client-example/README.md) is an example implementation of an SPA (runnning completely in the browser on web assembly) with bloom.
* [bloom-gtk-example](./bloom-gtk-example/README.md) is a todo app built with native GTK4 widgets.
* [bloom-server-example](./bloom-server-example/README.md) is an example implementation of using bloom to render html on the server.
