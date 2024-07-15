# async-content

This crate provides an easy way to provide context to async functions.

Use `provide_async_context` to provide the context:
```rust
provide_async_context(16, async {
    with_async_context(|my_number| {
        assert_eq!(my_number, 16);
    })
}).await
```