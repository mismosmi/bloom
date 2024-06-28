use futures_util::task::noop_waker;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) enum RunOrSuspendResult<T> {
    Suspend(Pin<Box<dyn Future<Output = T> + Send>>),
    Done(T),
}

pub(crate) fn run_or_suspend<T>(
    future: Pin<Box<dyn Future<Output = T> + Send>>,
) -> RunOrSuspendResult<T>
where
    T: 'static,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_or_suspend() {
        let future = async { 42 };
        let boxed = Box::pin(future);
        let result = run_or_suspend(boxed);

        match result {
            RunOrSuspendResult::Suspend(_) => panic!("Expected Done, got Suspend"),
            RunOrSuspendResult::Done(result) => assert_eq!(result, 42),
        }
    }
}
