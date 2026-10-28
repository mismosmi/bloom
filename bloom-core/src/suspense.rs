use futures_util::task::noop_waker;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Element;

/// Shows a fallback until every component below it has finished its first render.
/// While any of the children are still waiting on an async render, none of them are shown.
/// Once they are all done, the fallback is removed and the children are shown together.
///
/// The fallback is only shown on the first render.
/// When a child suspends again later on, its previous output stays on screen until the new one is ready.
///
/// Server-side rendering ignores suspense boundaries and always renders the children.
/// ```
/// use bloom_core::{component, Element, Suspense};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[component]
/// fn Profile() -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<div>"Profile"</div>))
/// }
///
/// #[component]
/// fn App() -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(
///         <Suspense fallback="Loading...">
///             <Profile />
///         </Suspense>
///     ))
/// }
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     App::builder().build().into(),
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>Profile</div>".to_string()));
/// ```
#[derive(bon::Builder)]
pub struct Suspense<N, E>
where
    N: From<String>,
{
    #[builder(into)]
    fallback: Element<N, E>,
    #[builder(default)]
    children: Vec<Element<N, E>>,
}

impl<N, E> From<Suspense<N, E>> for Element<N, E>
where
    N: From<String>,
{
    fn from(suspense: Suspense<N, E>) -> Self {
        Element::Suspense(Box::new(suspense.fallback), suspense.children)
    }
}

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
