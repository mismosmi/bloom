use crate::Element;

/// Groups elements without adding a node.
/// ```
/// use bloom_core::{keyed, Element, Fragment};
/// let group: Element<String, ()> = Fragment::builder()
///     .children(vec!["one".into(), "two".into()])
///     .build()
///     .into();
/// let keyed_group: Element<String, ()> = keyed(1, group);
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
