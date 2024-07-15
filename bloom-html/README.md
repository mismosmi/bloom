# bloom-html

bloom-html provides the central node-type for Browser-Environments:
`HtmlNode`.

It is part of the bloom UI-Framework and builds on the (https://crates.io/crates/bloom-core)[bloom-core] crate.

## The HtmlNode type
`HtmlNode` is roughly equivalent to a node in the Browser-DOM.

Currently implemented are
* `Element` which represents a tag such as `<div>` or `<span>`
* `Text` which represents some text such as the content of `<div>foo</div>`
* `Comment` which represents HTML-comments (`<!-- my comment here -->`)

## So what do I do with this?
For server-side rendering (which is, notably, stateless so no `use_state`, `use_effect` etc. here) take a look at [bloom-ssr](https://crates.io/crates/bloom-ssr)

For client-side rendering take a look at [bloom-client](https://crates.io/crates/bloom-client). It also supports hydrating from server-rendered html.

For anything more fancy please bring a little patience... 