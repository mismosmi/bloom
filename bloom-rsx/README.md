# bloom-rsx

Essentially, bloom-rsx implements rsx in a way that it just calls a builder-pattern based on the [builder-pattern](https://crates.io/crates/builder-pattern) crate.

## Tags
Lower-case tags will be transformed to calls to a `tag` function that must be in scope (bloom-html provides one for `HtmlNode`s):

```rust
rsx!(<div id="foo" on_click=|| {} />)
```
will be transformed into (the equivalent of)
```rust
tag("div")
    .attr("id", "foo")
    .on("click", || {})
    .build()
    .into()
```

## Children
Children are passed after building the tag itself:
```rust
rsx!(<div><span /></div>)
```
is transformed to
```rust
tag("div")
  .build()
  .children(vec![
    tag("span").build().into()
  ])
```

## Text
Text is just cast to the target node type using into:
```rust
rsx!(<div>"foobar"</div>)
```
becomes
```rust
tag("div")
  .build()
  .children(vec![
    "foobar".into()
  ])
```

## Root
A single root element is returned as is, multiple roots are wrapped in a fragment:
```rust
rsx!(<div /><span />)
```
becomes
```rust
vec![tag("div").build().into(), tag("span").build().into()].into()
```

## Keys
`key` identifies an element among its siblings so it keeps its state when its list is reordered.
A tag or component with a `key` is wrapped in a keyed element instead of passing it on as an attribute or prop:
```rust
rsx!(<li key={item.id} />)
rsx!(<Row key={item.id} />)
```
becomes
```rust
keyed(item.id, tag("li").build())
keyed(item.id, Row::builder().build())
```
Like `tag`, a `keyed` function must be in scope (bloom-html's prelude re-exports the one from bloom-core).
`bloom_core::Fragment` is a component too, so a keyed `<Fragment>` keys several elements at once:
```rust
use bloom_core::Fragment;

rsx!(<Fragment key={item.id}><dt /><dd /></Fragment>)
```

## Components
Uppercase tags are transformed to a builder pattern:
```rust
rsx!(<MyComponent foo="bar"><div /></MyComponent>)
```
becomes
```rust
MyComponent::builder()
  .foo("bar")
  .children(vec![
    tag("div").build().into()
  ])
  .build()
  .into()
```