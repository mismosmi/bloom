use std::{any::Any, cell::RefCell, mem, rc::Rc, sync::Arc};

use crate::{
    context::ContextMap,
    effect::{Cleanup, Effect},
    Element,
};

pub(crate) struct RenderContext<N> {
    pub(crate) parent: Arc<N>,
    pub(crate) sibling: Option<Arc<N>>,
    pub(crate) context: ContextMap,
}

impl<N> Clone for RenderContext<N> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            sibling: self.sibling.clone(),
            context: self.context.clone(),
        }
    }
}

impl<N> RenderContext<N> {
    pub(crate) fn new(parent: Arc<N>, sibling: Option<Arc<N>>, context: ContextMap) -> Self {
        Self {
            parent,
            sibling,
            context,
        }
    }

    pub(crate) fn with_parent(&self, parent: Arc<N>) -> Self {
        Self {
            parent,
            sibling: None,
            context: self.context.clone(),
        }
    }

    pub(crate) fn with_parent_and_sibling(&self, parent: Arc<N>, sibling: Option<Arc<N>>) -> Self {
        Self {
            parent,
            sibling,
            context: self.context.clone(),
        }
    }

    pub(crate) fn with_sibling(&self, sibling: Option<Arc<N>>) -> Self {
        Self {
            parent: self.parent.clone(),
            sibling,
            context: self.context.clone(),
        }
    }

    pub(crate) fn with_context(&self, value: Arc<dyn Any + Send + Sync>) -> Self {
        let mut new_context = self.context.as_ref().clone();
        new_context.insert((*value).type_id(), value);
        Self {
            parent: self.parent.clone(),
            sibling: self.sibling.clone(),
            context: Arc::new(new_context),
        }
    }

    pub(crate) fn with_sibling_and_context(
        &self,
        sibling: Option<Arc<N>>,
        value: Arc<dyn Any + Send + Sync>,
    ) -> Self {
        let mut new_context = self.context.as_ref().clone();
        new_context.insert((*value).type_id(), value);
        Self {
            parent: self.parent.clone(),
            sibling,
            context: Arc::new(new_context),
        }
    }
}

pub(crate) enum RenderQueueItem<N, E, TN>
where
    N: From<String>,
{
    Create {
        current: *mut TN,
        ctx: RenderContext<N>,
    },
    Reload {
        current: *mut TN,
        ctx: RenderContext<N>,
    },
    Update {
        current: *mut TN,
        next: Element<N, E>,
        ctx: RenderContext<N>,
    },
    Remove {
        current: TN,
        parent: Arc<N>,
    },
}

/// The cleanups of a component's effects, kept from one render pass to the next
#[derive(Default)]
pub(crate) struct ComponentEffects {
    cleanups: Vec<(u64, Cleanup)>,
    unmounted: bool,
}

pub(crate) type EffectSlot = Rc<RefCell<ComponentEffects>>;

pub(crate) struct RenderQueue<N, E, TN>
where
    N: From<String>,
{
    queue: Vec<RenderQueueItem<N, E, TN>>,
    effects: Vec<(EffectSlot, Vec<(u64, Effect)>)>,
    unmounted: Vec<EffectSlot>,
}

impl<N, E, TN> RenderQueue<N, E, TN>
where
    N: From<String>,
{
    pub(crate) fn new() -> Self {
        Self {
            queue: Vec::new(),
            effects: Vec::new(),
            unmounted: Vec::new(),
        }
    }

    pub(crate) fn create(&mut self, current: &mut TN, ctx: RenderContext<N>) {
        self.queue.push(RenderQueueItem::Create {
            current: current as *mut TN,
            ctx,
        })
    }

    pub(crate) fn reload(&mut self, current: &mut TN, ctx: RenderContext<N>) {
        self.queue.push(RenderQueueItem::Reload { current, ctx })
    }

    pub(crate) fn update(&mut self, current: &mut TN, next: Element<N, E>, ctx: RenderContext<N>) {
        self.queue.push(RenderQueueItem::Update {
            current: current as *mut TN,
            next,
            ctx,
        })
    }

    pub(crate) fn remove(&mut self, current: TN, parent: Arc<N>) {
        self.queue.push(RenderQueueItem::Remove { current, parent })
    }

    pub(crate) fn next(&mut self) -> Option<RenderQueueItem<N, E, TN>> {
        self.queue.pop()
    }

    pub(crate) fn queue_effects(&mut self, slot: &EffectSlot, effects: Vec<(u64, Effect)>) {
        self.effects.push((Rc::clone(slot), effects));
    }

    /// Runs the cleanups of an unmounted component after the render pass
    pub(crate) fn unmount(&mut self, slot: &EffectSlot) {
        self.unmounted.push(Rc::clone(slot));
    }

    pub(crate) fn run_effects(&mut self) {
        for slot in self.unmounted.drain(..) {
            let mut slot = slot.borrow_mut();
            slot.unmounted = true;
            for (_, cleanup) in slot.cleanups.drain(..) {
                cleanup.run();
            }
        }

        for (slot, effects) in self.effects.drain(..) {
            let mut slot = slot.borrow_mut();
            if slot.unmounted {
                continue;
            }

            let mut cleanups = mem::take(&mut slot.cleanups).into_iter();
            let mut next_cleanups = Vec::with_capacity(effects.len());
            for (effect_hash, effect) in effects {
                match cleanups.next() {
                    // The arguments haven't changed, so the effect doesn't run again
                    Some((cleanup_hash, cleanup)) if cleanup_hash == effect_hash => {
                        next_cleanups.push((cleanup_hash, cleanup));
                    }
                    Some((_, cleanup)) => {
                        cleanup.run();
                        next_cleanups.push((effect_hash, effect.run()));
                    }
                    None => next_cleanups.push((effect_hash, effect.run())),
                }
            }
            for (_, cleanup) in cleanups {
                cleanup.run();
            }
            slot.cleanups = next_cleanups;
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
            // boxed so node addresses stay stable while the Vec grows
            #[allow(clippy::vec_box)]
            children: Vec<Box<TreeNode>>,
        }

        let mut root = TreeNode {
            children: Vec::new(),
        };

        let mut child = Box::new(TreeNode {
            children: Vec::new(),
        });

        let mut queue = RenderQueue::<String, (), TreeNode>::new();

        queue.reload(
            &mut root,
            RenderContext::new(Arc::new("".to_string()), None, Arc::default()),
        );

        let item = queue.next().unwrap();

        match item {
            RenderQueueItem::Reload { current, .. } => {
                assert_eq!(current as *const _, &root as *const _);

                queue.create(
                    child.as_mut(),
                    RenderContext::new(Arc::new("".to_string()), None, Arc::default()),
                );
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
