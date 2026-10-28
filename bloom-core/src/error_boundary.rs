use std::sync::Arc;

use crate::Element;

/// Builds the element an [ErrorBoundary] shows in place of its children from their error
pub(crate) type Fallback<N, E> = Arc<dyn Fn(E) -> Element<N, E> + Send + Sync>;

/// Catches the errors of the components below it.
/// When one of them returns an error, all children are removed and the fallback is shown
/// in their place, built from the error. Errors of the fallback are passed on to the next
/// boundary above.
///
/// The fallback stays until the boundary is created again, e.g. by changing its key.
/// When several children fail in the same render pass, the first one in document order is shown.
/// ```
/// use bloom_core::{component, Element, ErrorBoundary};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[component]
/// fn Profile() -> Result<Element<HtmlNode, String>, String> {
///     Err("Profile not found".to_string())
/// }
///
/// #[component]
/// fn App() -> Result<Element<HtmlNode, String>, String> {
///     Ok(rsx!(
///         <ErrorBoundary fallback={|error: String| rsx!(<div>{error}</div>)}>
///             <Profile />
///         </ErrorBoundary>
///     ))
/// }
///
/// // Without rsx
/// let app: Element<HtmlNode, String> = ErrorBoundary::builder()
///     .fallback(|error: String| error.into())
///     .children(vec![rsx!(<Profile />)])
///     .build()
///     .into();
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     App::builder().build().into(),
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok("<div>Profile not found</div>".to_string()));
/// ```
///
/// An error boundary needs a fallback:
/// ```compile_fail
/// use bloom_core::{Element, ErrorBoundary};
/// use bloom_html::HtmlNode;
///
/// let boundary: Element<HtmlNode, ()> = ErrorBoundary::builder().build().into();
/// ```
pub struct ErrorBoundary<N, E>
where
    N: From<String>,
{
    fallback: Fallback<N, E>,
    children: Vec<Element<N, E>>,
}

impl<N, E> ErrorBoundary<N, E>
where
    N: From<String>,
{
    pub fn builder() -> ErrorBoundaryBuilder<N, E, ()> {
        ErrorBoundaryBuilder {
            fallback: (),
            children: Vec::new(),
        }
    }
}

pub struct ErrorBoundaryBuilder<N, E, F>
where
    N: From<String>,
{
    fallback: F,
    children: Vec<Element<N, E>>,
}

impl<N, E> ErrorBoundaryBuilder<N, E, ()>
where
    N: From<String>,
{
    pub fn fallback<F>(self, fallback: F) -> ErrorBoundaryBuilder<N, E, Fallback<N, E>>
    where
        F: Fn(E) -> Element<N, E> + Send + Sync + 'static,
    {
        ErrorBoundaryBuilder {
            fallback: Arc::new(fallback),
            children: self.children,
        }
    }
}

impl<N, E, F> ErrorBoundaryBuilder<N, E, F>
where
    N: From<String>,
{
    pub fn children(mut self, children: Vec<Element<N, E>>) -> Self {
        self.children = children;
        self
    }
}

impl<N, E> ErrorBoundaryBuilder<N, E, Fallback<N, E>>
where
    N: From<String>,
{
    pub fn build(self) -> ErrorBoundary<N, E> {
        ErrorBoundary {
            fallback: self.fallback,
            children: self.children,
        }
    }
}

impl<N, E> From<ErrorBoundary<N, E>> for Element<N, E>
where
    N: From<String>,
{
    fn from(boundary: ErrorBoundary<N, E>) -> Self {
        Element::ErrorBoundary(boundary.fallback, boundary.children)
    }
}
