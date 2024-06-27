use std::sync::Arc;

use crate::Element;

pub(crate) enum RenderQueueItem<N, E, TN> {
    Create {
        current: *mut TN,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    },
    Reload {
        current: *mut TN,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    },
    Update {
        current: *mut TN,
        next: Element<N, E>,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    },
    Remove {
        current: TN,
        parent: Arc<N>,
    },
}

pub(crate) struct RenderQueue<N, E, TN>(Vec<RenderQueueItem<N, E, TN>>);

impl<N, E, TN> RenderQueue<N, E, TN> {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn create(&mut self, current: &mut TN, parent: Arc<N>, sibling: Option<Arc<N>>) {
        self.0.push(RenderQueueItem::Create {
            current: current as *mut TN,
            parent,
            sibling,
        })
    }

    pub(crate) fn reload(&mut self, current: &mut TN, parent: Arc<N>, sibling: Option<Arc<N>>) {
        self.0.push(RenderQueueItem::Reload {
            current,
            parent,
            sibling,
        })
    }

    pub(crate) fn update(
        &mut self,
        current: &mut TN,
        next: Element<N, E>,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    ) {
        self.0.push(RenderQueueItem::Update {
            current: current as *mut TN,
            next,
            parent,
            sibling,
        })
    }

    pub(crate) fn remove(&mut self, current: TN, parent: Arc<N>) {
        self.0.push(RenderQueueItem::Remove { current, parent })
    }

    pub(crate) fn next(&mut self) -> Option<RenderQueueItem<N, E, TN>> {
        self.0.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{borrow::Borrow, sync::Arc};

    #[test]
    fn basic_render_queue() {
        struct TreeNode {
            children: Vec<Box<TreeNode>>,
        }

        let mut root = TreeNode {
            children: Vec::new(),
        };

        let mut child = Box::new(TreeNode {
            children: Vec::new(),
        });

        let mut queue = RenderQueue::<(), (), TreeNode>::new();

        queue.reload(&mut root, Arc::new(()), None);

        let item = queue.next().unwrap();

        match item {
            RenderQueueItem::Reload { current, .. } => {
                assert_eq!(current as *const _, &root as *const _);

                queue.create(child.as_mut(), Arc::new(()), None);
                unsafe {
                    (&mut *current).children.push(child);
                }
            }
            _ => panic!("Unexpected item"),
        }

        let item = queue.next().unwrap();

        match item {
            RenderQueueItem::Create { current, .. } => {
                assert_eq!(current as *const _, &*root.children[0] as *const _);
            }
            _ => panic!("Unexpected item"),
        }
    }
}
