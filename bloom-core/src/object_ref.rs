use std::sync::Arc;

use async_context::with_async_context_mut;

use crate::hook::Hook;

pub fn use_ref<T>() -> Arc<T>
where
    T: Default + Send + Sync + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Arc::new(T::default()));
            hook.ref_index += 1;
            object_ref
                .clone()
                .downcast()
                .expect("Hook Invariant Violation: Failed to cast ref")
        } else {
            Arc::new(T::default())
        }
    })
}
