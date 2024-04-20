//! A way to have some context within async functions. This can be used to implement React-like hooks.
//!
#![feature(local_key_cell_methods)]
#![feature(box_into_inner)]

use core::future::Future;
use std::{any::Any, cell::RefCell, pin::Pin};

use pin_project::pin_project;

thread_local! {
    static CTX: RefCell<Box<dyn Any>> = RefCell::new(Box::new(()));
}

/// Stores a future along with the async context provided for it.
/// Create AsyncContext using [provide_async_context]
/// Access the context using [with_async_context] or [with_async_context_mut]
#[pin_project]
pub struct AsyncContext<C, T, F>
where
    C: 'static,
    F: Future<Output = T>,
{
    ctx: RefCell<Option<C>>,
    #[pin]
    future: F,
}

/// Wraps a future with some async context.
/// Within the future, the provided context can be retrieved using [with_async_context] or [with_async_context_mut]
pub fn provide_async_context<C, T, F>(ctx: C, future: F) -> AsyncContext<C, T, F>
where
    C: 'static,
    F: Future<Output = T>,
{
    AsyncContext {
        ctx: RefCell::new(Some(ctx)),
        future,
    }
}

impl<C, T, F> Future for AsyncContext<C, T, F>
where
    F: Future<Output = T>,
{
    type Output = T;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let ctx: C = self.ctx.borrow_mut().take().unwrap();
        CTX.set(Box::new(ctx));
        let projection = self.project();
        let future: Pin<&mut F> = projection.future;
        let poll = future.poll(cx);
        let ctx: C = Box::into_inner(CTX.replace(Box::new(())).downcast().unwrap());
        projection.ctx.replace(Some(ctx));
        poll
    }
}

/// Retrieves immutable ref for async context in order to read values.
pub fn with_async_context<C, F, R>(f: F) -> R
where
    F: FnOnce(&C) -> R,
    C: 'static,
{
    return CTX.with(|value| f(value.borrow().downcast_ref::<C>().unwrap()));
}

/// Retrieves mutable ref for async context in order to read values.
pub fn with_async_context_mut<C, F, R>(f: F) -> R
where
    F: FnOnce(&mut C) -> R,
    C: 'static,
{
    return CTX.with(|value| f(value.borrow_mut().downcast_mut::<C>().unwrap()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_works() {
        async fn runs_with_context() -> String {
            let value = with_async_context(|value: &String| value.clone());
            value
        }

        let async_context = provide_async_context("foobar".to_string(), runs_with_context());

        let value = async_context.await;

        assert_eq!("foobar", value);
    }
}
