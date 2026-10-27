use crate::Element;

/// Groups elements without adding a node.
/// In rsx it is used like a component, and like any other element it can be keyed,
/// e.g. to key several nodes at once:
/// ```
/// use bloom_core::{Element, Fragment};
/// use bloom_html::{prelude::*, HtmlNode};
/// use bloom_rsx::rsx;
///
/// let items = vec![(1, "one"), (2, "two")];
/// let list: Element<HtmlNode, ()> = rsx!(
///     <dl>{items.iter().map(|(id, label)| rsx!(
///         <Fragment key={id}><dt>{id.to_string()}</dt><dd>{*label}</dd></Fragment>
///     )).collect::<Vec<_>>()}</dl>
/// );
///
/// // Without rsx
/// let group: Element<HtmlNode, ()> = Fragment::builder().children(vec!["one".into()]).build().into();
/// ```
pub struct Fragment<N, E>
where
    N: From<String>,
{
    children: Vec<Element<N, E>>,
}

impl<N, E> Fragment<N, E>
where
    N: From<String>,
{
    pub fn builder() -> FragmentBuilder<N, E> {
        FragmentBuilder {
            children: Vec::new(),
        }
    }
}

pub struct FragmentBuilder<N, E>
where
    N: From<String>,
{
    children: Vec<Element<N, E>>,
}

impl<N, E> FragmentBuilder<N, E>
where
    N: From<String>,
{
    pub fn children(mut self, children: Vec<Element<N, E>>) -> Self {
        self.children = children;
        self
    }

    pub fn build(self) -> Fragment<N, E> {
        Fragment {
            children: self.children,
        }
    }
}

impl<N, E> From<Fragment<N, E>> for Element<N, E>
where
    N: From<String>,
{
    fn from(fragment: Fragment<N, E>) -> Self {
        Element::Fragment(fragment.children)
    }
}
