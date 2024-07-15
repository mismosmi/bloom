# bloom-rsx

Essentially, bloom-rsx implements rsx in a way that it just calls a builder-pattern based on the (https://crates.io/crates/builder-pattern)[builder-pattern] crate.

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

## Components
Uppercase tags are transformed to a builder pattern:
```rust
rsx!(<MyComponent foo="bar"><div /></MyComponent>)
```
becomes
```rust
MyComponent::new()
  .foo("bar")
  .children(vec![
    tag("div").build().into()
  ])
  .build()
  .into()
```