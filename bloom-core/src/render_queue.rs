use std::{collections::HashMap, sync::Arc};

use crate::{
    component::AnyComponent,
    effect::{Cleanup, Effect},
    Element,
};

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

pub(crate) struct RenderQueue<N, E, TN> {
    queue: Vec<RenderQueueItem<N, E, TN>>,
    effects: HashMap<*const (), Vec<(u64, Effect)>>,
    cleanups: HashMap<*const (), Vec<(u64, Cleanup)>>,
    clear_cleanups: Vec<*const ()>,
}

impl<N, E, TN> RenderQueue<N, E, TN> {
    pub(crate) fn new() -> Self {
        Self {
            queue: Vec::new(),
            effects: HashMap::new(),
            cleanups: HashMap::new(),
            clear_cleanups: Vec::new(),
        }
    }

    pub(crate) fn create(&mut self, current: &mut TN, parent: Arc<N>, sibling: Option<Arc<N>>) {
        self.queue.push(RenderQueueItem::Create {
            current: current as *mut TN,
            parent,
            sibling,
        })
    }

    pub(crate) fn reload(&mut self, current: &mut TN, parent: Arc<N>, sibling: Option<Arc<N>>) {
        self.queue.push(RenderQueueItem::Reload {
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
        self.queue.push(RenderQueueItem::Update {
            current: current as *mut TN,
            next,
            parent,
            sibling,
        })
    }

    pub(crate) fn remove(&mut self, current: TN, parent: Arc<N>) {
        self.queue.push(RenderQueueItem::Remove { current, parent })
    }

    pub(crate) fn next(&mut self) -> Option<RenderQueueItem<N, E, TN>> {
        self.queue.pop()
    }

    pub(crate) fn queue_effects(
        &mut self,
        component: &Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
        effects: Vec<(u64, Effect)>,
    ) {
        self.effects.insert(
            component.as_ref() as *const dyn AnyComponent<Node = N, Error = E> as *const (),
            effects,
        );
    }

    pub(crate) fn queue_cleanups(
        &mut self,
        component: &Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    ) {
        self.clear_cleanups
            .push(component.as_ref() as *const dyn AnyComponent<Node = N, Error = E> as *const ());
    }

    pub(crate) fn move_cleanups(
        &mut self,
        old_component: &Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
        new_component: &Arc<dyn AnyComponent<Node = N, Error = E> + Send + Sync>,
    ) {
        if let Some(cleanups) = self.cleanups.remove(
            &(old_component.as_ref() as *const dyn AnyComponent<Node = N, Error = E> as *const ()),
        ) {
            self.cleanups.insert(
                new_component.as_ref() as *const dyn AnyComponent<Node = N, Error = E> as *const (),
                cleanups,
            );
        }
    }

    pub(crate) fn run_effects(&mut self) {
        for component in self.clear_cleanups.drain(..) {
            if let Some(cleanups) = self.cleanups.remove(&component) {
                for (_, cleanup) in cleanups {
                    cleanup.run()
                }
            }
        }

        for (component, effects) in self.effects.drain() {
            let mut next_cleanups = Vec::with_capacity(effects.len());
            if let Some(cleanups) = self.cleanups.remove(&component) {
                for ((effect_hash, effect), (cleanup_hash, cleanup)) in
                    effects.into_iter().zip(cleanups.into_iter())
                {
                    if effect_hash == cleanup_hash {
                        next_cleanups.push((cleanup_hash, cleanup));
                    } else {
                        cleanup.run();
                        next_cleanups.push((effect_hash, effect.run()));
                    }
                }
            } else {
                for (effect_hash, effect) in effects {
                    next_cleanups.push((effect_hash, effect.run()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
