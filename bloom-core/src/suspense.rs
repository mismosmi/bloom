use noop_waker::noop_waker;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) enum RunOrSuspendResult<T> {
    Suspend(Pin<Box<dyn Future<Output = T> + Send>>),
    Done(T),
}

pub(crate) fn run_or_suspend<T, F>(future: F) -> RunOrSuspendResult<T>
where
    F: Future<Output = T> + Send + 'static,
{
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut boxed = Box::pin(future);
    let poll = Future::poll(boxed.as_mut(), &mut cx);

    match poll {
        Poll::Pending => RunOrSuspendResult::Suspend(boxed),
        Poll::Ready(result) => RunOrSuspendResult::Done(result),
    }
}
