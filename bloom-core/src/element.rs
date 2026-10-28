use std::{any::Any, hash::Hash, sync::Arc};

use crate::{
    component::{AnyComponent, ComponentDiff},
    Key,
};

/// The element type is returned from component render-functions.
/// It can be constructed from a Node-type, e.G. HtmlNode, or a Component.
/// ```
/// use bloom_core::Element;
/// use bloom_html::HtmlNode;
///
/// let element: Element<HtmlNode, ()> = HtmlNode::element("div").attr("id", "test123").build().into();
/// ```
pub enum Element<Node, Error>
where
    Node: From<String>,
{
    Component(Arc<dyn AnyComponent<Node = Node, Error = Error> + Send + Sync + 'static>),
    Node(Node, Vec<Element<Node, Error>>),
    Fragment(Vec<Element<Node, Error>>),
    Provider(Arc<dyn Any + Send + Sync>, Vec<Element<Node, Error>>),
    /// Identifies the element among its siblings, see [keyed].
    Keyed(Key, Box<Element<Node, Error>>),
    Suspense(Box<Element<Node, Error>>, Vec<Element<Node, Error>>),
    /// Shows the element built by the function from the error of a child, see [crate::ErrorBoundary].
    ErrorBoundary(
        Arc<dyn Fn(crate::CaughtError<Error>) -> Element<Node, Error> + Send + Sync>,
        Vec<Element<Node, Error>>,
    ),
}

impl<N, E> Element<N, E>
where
    N: From<String>,
{
    pub fn fragment(children: Vec<Element<N, E>>) -> Self {
        Self::Fragment(children)
    }
}

/// A keyed element keeps its state and native nodes when it is moved within a list,
/// and is rebuilt from scratch when its key changes.
/// rsx calls this function for every tag or component with a `key`, so it has to be in scope.
/// `bloom_html::prelude` re-exports it next to `tag`:
/// ```
/// use bloom_core::{component, Element, Fragment};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// #[component]
/// fn Row(label: String) -> Result<Element<HtmlNode, ()>, ()> {
///     Ok(rsx!(<li>{label.clone()}</li>))
/// }
///
/// let items = vec![(1, "one"), (2, "two")];
/// let lists: Element<HtmlNode, ()> = rsx!(
///     <ul>{items.iter().map(|(id, label)| rsx!(<li key={id}>{*label}</li>)).collect::<Vec<_>>()}</ul>
///     <ul>{items.iter().map(|(id, label)| rsx!(<Row key={id} label=*label />)).collect::<Vec<_>>()}</ul>
///     <dl>{items.iter().map(|(id, label)| rsx!(
///         <Fragment key={id}><dt>{id.to_string()}</dt><dd>{*label}</dd></Fragment>
///     )).collect::<Vec<_>>()}</dl>
/// );
/// # let html = futures_executor::block_on(bloom_ssr::render_to_string(
/// #     lists,
/// #     futures_executor::ThreadPool::new().unwrap(),
/// # ));
/// # assert_eq!(html, Ok(concat!(
/// #     "<ul><li>one</li><li>two</li></ul>",
/// #     "<ul><li>one</li><li>two</li></ul>",
/// #     "<dl><dt>1</dt><dd>one</dd><dt>2</dt><dd>two</dd></dl>",
/// # ).to_string()));
///
/// // Without rsx
/// let item: Element<HtmlNode, ()> = keyed(1, tag("li").build().children(vec!["one".into()]));
/// ```
pub fn keyed<N, E, K, T>(key: K, element: T) -> Element<N, E>
where
    N: From<String>,
    K: Hash,
    T: Into<Element<N, E>>,
{
    Element::Keyed(Key::new(key), Box::new(element.into()))
}

impl<N, E> From<Vec<Element<N, E>>> for Element<N, E>
where
    N: From<String>,
{
    fn from(children: Vec<Element<N, E>>) -> Self {
        Element::Fragment(children)
    }
}

impl<N, E> From<String> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: String) -> Self {
        Element::Node(N::from(value), vec![])
    }
}

impl<N, E> From<&str> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: &str) -> Self {
        Element::Node(N::from(value.to_string()), vec![])
    }
}

impl<N, E> From<&String> for Element<N, E>
where
    N: From<String>,
{
    fn from(value: &String) -> Self {
        Element::Node(N::from(value.to_string()), vec![])
    }
}

impl<N, E> From<()> for Element<N, E>
where
    N: From<String>,
{
    fn from(_: ()) -> Self {
        Element::Fragment(Vec::new())
    }
}

impl<N, E> PartialEq for Element<N, E>
where
    N: From<String> + PartialEq + 'static,
    E: 'static,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Element::Component(a), Element::Component(b)) => a.compare(b) == ComponentDiff::Equal,
            (Element::Node(a, ac), Element::Node(b, bc)) => a == b && ac == bc,
            (Element::Fragment(ac), Element::Fragment(bc)) => ac == bc,
            (Element::Provider(av, ac), Element::Provider(bv, bc)) => {
                Arc::ptr_eq(av, bv) && ac == bc
            }
            (Element::Keyed(ak, a), Element::Keyed(bk, b)) => ak == bk && a == b,
            (Element::Suspense(af, ac), Element::Suspense(bf, bc)) => af == bf && ac == bc,
            (Element::ErrorBoundary(af, ac), Element::ErrorBoundary(bf, bc)) => {
                Arc::ptr_eq(af, bf) && ac == bc
            }
            _ => false,
        }
    }
}

impl<N, E> Clone for Element<N, E>
where
    N: From<String> + Clone,
{
    fn clone(&self) -> Self {
        match self {
            Element::Component(component) => Element::Component(component.clone()),
            Element::Fragment(children) => Element::Fragment(children.clone()),
            Element::Node(node, children) => Element::Node(node.clone(), children.clone()),
            Element::Provider(value, children) => {
                Element::Provider(value.clone(), children.clone())
            }
            Element::Keyed(key, element) => Element::Keyed(*key, element.clone()),
            Element::Suspense(fallback, children) => {
                Element::Suspense(fallback.clone(), children.clone())
            }
            Element::ErrorBoundary(fallback, children) => {
                Element::ErrorBoundary(fallback.clone(), children.clone())
            }
        }
    }
}
