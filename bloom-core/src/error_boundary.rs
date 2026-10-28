use std::{
    fmt,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_channel::Sender;

use crate::Element;

/// Builds the element an [ErrorBoundary] shows in place of its children from their error
pub(crate) type Fallback<N, E> = Arc<dyn Fn(CaughtError<E>) -> Element<N, E> + Send + Sync>;

/// The error an [ErrorBoundary] passes to its fallback.
/// It dereferences to the error of the child and is cheap to clone,
/// so it can be used in the fallback and moved into event handlers at the same time.
pub struct CaughtError<E> {
    error: Arc<E>,
    dismiss: Option<Dismiss>,
}

/// Asks the render loop to show the children of a boundary again
#[derive(Clone)]
pub(crate) struct Dismiss {
    /// Set once the error is dismissed, read by the boundary
    pub(crate) dismissed: Arc<AtomicBool>,
    /// Makes the component that rendered the boundary render again
    pub(crate) owner: Option<Arc<AtomicBool>>,
    pub(crate) signal: Sender<()>,
}

impl<E> CaughtError<E> {
    pub(crate) fn new(error: E, dismiss: Option<Dismiss>) -> Self {
        Self {
            error: Arc::new(error),
            dismiss,
        }
    }

    /// Removes the fallback and shows the children again.
    /// The component that rendered the boundary renders again, so the boundary gets new
    /// children, which are created from scratch.
    ///
    /// A boundary at the root of the render loop has no component to render it again,
    /// and server-side rendering never shows the children again,
    /// so dismissing does nothing there.
    pub fn dismiss(&self) {
        if let Some(dismiss) = &self.dismiss {
            dismiss.dismissed.store(true, Ordering::SeqCst);
            if let Some(owner) = &dismiss.owner {
                owner.store(true, Ordering::SeqCst);
            }
            let _ = dismiss.signal.try_send(());
        }
    }

    /// The error of the child, shared by all clones
    pub fn error(&self) -> &Arc<E> {
        &self.error
    }
}

impl<E> Deref for CaughtError<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl<E> Clone for CaughtError<E> {
    fn clone(&self) -> Self {
        Self {
            error: self.error.clone(),
            dismiss: self.dismiss.clone(),
        }
    }
}

/// Clones of the same caught error are equal, so it can be passed to components as a prop
impl<E> PartialEq for CaughtError<E> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.error, &other.error)
    }
}

impl<E: fmt::Debug> fmt::Debug for CaughtError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl<E: fmt::Display> fmt::Display for CaughtError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

/// Catches the errors of the components below it.
/// When one of them returns an error, all children are removed and the fallback is shown
/// in their place, built from the [CaughtError]. Errors of the fallback are passed on to the next
/// boundary above.
///
/// The fallback stays until the error is dismissed with [CaughtError::dismiss] or the boundary
/// is created again, e.g. by changing its key.
/// When several children fail in the same render pass, the first one in document order is shown.
/// ```
/// use bloom_core::{component, CaughtError, Element, ErrorBoundary};
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
///         <ErrorBoundary fallback={|error: CaughtError<String>| rsx!(
///             <div>{error.as_str()}</div>
///             <button on_click={move |_| error.dismiss()}>"Try again"</button>
///         )}>
///             <Profile />
///         </ErrorBoundary>
///     ))
/// }
///
/// // Without rsx
/// let app: Element<HtmlNode, String> = ErrorBoundary::builder()
///     .fallback(|error: CaughtError<String>| error.to_string().into())
///     .children(vec![rsx!(<Profile />)])
///     .build()
///     .into();
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     App::builder().build().into(),
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(
/// #     html,
/// #     Ok("<div>Profile not found</div><button>Try again</button>".to_string())
/// # );
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
        F: Fn(CaughtError<E>) -> Element<N, E> + Send + Sync + 'static,
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
