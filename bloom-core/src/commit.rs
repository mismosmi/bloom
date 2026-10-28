use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    effect::{Cleanup, Effect},
    ObjectModel,
};

/// Identifies a component across render passes
pub(crate) type ComponentId = u64;

/// A change to the object model, applied when the render pass is committed
pub(crate) enum NodeOperation<N> {
    Create {
        node: Arc<N>,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    },
    Remove {
        node: Arc<N>,
        parent: Arc<N>,
    },
    Move {
        node: Arc<N>,
        parent: Arc<N>,
        sibling: Option<Arc<N>>,
    },
    Update {
        node: Arc<N>,
        next: Arc<N>,
    },
}

/// Everything a subtree needs to commit, in document order
pub(crate) struct Changes<N> {
    operations: Vec<NodeOperation<N>>,
    effects: Vec<(ComponentId, Vec<(u64, Effect)>)>,
    unmounted: Vec<ComponentId>,
}

impl<N> Default for Changes<N> {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            effects: Vec::new(),
            unmounted: Vec::new(),
        }
    }
}

impl<N> Changes<N> {
    /// Adds the changes of a subtree that comes after everything already collected
    pub(crate) fn append(&mut self, mut other: Changes<N>) {
        self.operations.append(&mut other.operations);
        self.effects.append(&mut other.effects);
        self.unmounted.append(&mut other.unmounted);
    }

    pub(crate) fn create_node(&mut self, node: &Arc<N>, parent: &Arc<N>, sibling: &Option<Arc<N>>) {
        self.operations.push(NodeOperation::Create {
            node: Arc::clone(node),
            parent: Arc::clone(parent),
            sibling: sibling.clone(),
        })
    }

    pub(crate) fn remove_node(&mut self, node: &Arc<N>, parent: &Arc<N>) {
        self.operations.push(NodeOperation::Remove {
            node: Arc::clone(node),
            parent: Arc::clone(parent),
        })
    }

    pub(crate) fn move_node(&mut self, node: &Arc<N>, parent: &Arc<N>, sibling: &Option<Arc<N>>) {
        self.operations.push(NodeOperation::Move {
            node: Arc::clone(node),
            parent: Arc::clone(parent),
            sibling: sibling.clone(),
        })
    }

    pub(crate) fn update_node(&mut self, node: &Arc<N>, next: &Arc<N>) {
        self.operations.push(NodeOperation::Update {
            node: Arc::clone(node),
            next: Arc::clone(next),
        })
    }

    pub(crate) fn queue_effects(&mut self, component: ComponentId, effects: Vec<(u64, Effect)>) {
        self.effects.push((component, effects));
    }

    /// Runs the cleanups of a removed component when the render pass is committed
    pub(crate) fn unmount(&mut self, component: ComponentId) {
        self.unmounted.push(component);
    }
}

/// The cleanups of each component's effects, kept from one render pass to the next.
/// They stay with the render loop because cleanups don't have to be `Send`.
#[derive(Default)]
pub(crate) struct Cleanups(HashMap<ComponentId, Vec<(u64, Cleanup)>>);

impl Cleanups {
    /// Applies a render pass in one go without yielding in between:
    /// first all cleanups run, then the object model is updated and finally the effects run.
    pub(crate) fn commit<N, P>(&mut self, changes: Changes<N>, object_model: &mut P)
    where
        P: ObjectModel<Node = N>,
    {
        for component in &changes.unmounted {
            for (_, cleanup) in self.0.remove(component).into_iter().flatten() {
                cleanup.run();
            }
        }
        let unmounted: HashSet<ComponentId> = changes.unmounted.into_iter().collect();

        // The effects whose argument changed, they run once the object model is updated
        let mut pending = Vec::new();
        for (component, effects) in changes.effects {
            if unmounted.contains(&component) {
                continue;
            }

            let mut cleanups = self.0.remove(&component).unwrap_or_default().into_iter();
            let mut next_cleanups = Vec::with_capacity(effects.len());
            let mut changed = Vec::new();
            for (index, (effect_hash, effect)) in effects.into_iter().enumerate() {
                match cleanups.next() {
                    // The arguments haven't changed, so the effect doesn't run again
                    Some((cleanup_hash, cleanup)) if cleanup_hash == effect_hash => {
                        next_cleanups.push((cleanup_hash, cleanup));
                        continue;
                    }
                    Some((_, cleanup)) => cleanup.run(),
                    None => {}
                }
                // Replaced by the effect's cleanup once it has run
                next_cleanups.push((effect_hash, Cleanup::from(())));
                changed.push((index, effect));
            }
            for (_, cleanup) in cleanups {
                cleanup.run();
            }
            self.0.insert(component, next_cleanups);
            pending.push((component, changed));
        }

        for operation in changes.operations {
            match operation {
                NodeOperation::Create {
                    node,
                    parent,
                    sibling,
                } => object_model.create(&node, &parent, &sibling),
                NodeOperation::Remove { node, parent } => object_model.remove(&node, &parent),
                NodeOperation::Move {
                    node,
                    parent,
                    sibling,
                } => object_model.move_node(&node, &parent, &sibling),
                NodeOperation::Update { node, next } => object_model.update(&node, &next),
            }
        }

        for (component, changed) in pending {
            for (index, effect) in changed {
                let cleanup = effect.run();
                if let Some(cleanups) = self.0.get_mut(&component) {
                    cleanups[index].1 = cleanup;
                }
            }
        }
    }
}
