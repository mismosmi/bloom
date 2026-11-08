use crate::{MaybeSend, MaybeSync};
use std::sync::Arc;

use async_context::with_async_context_mut;

use crate::hook::Hook;

/// use_ref can be used to obtain a persistent reference to an object.
/// The object returned from ref is guaranteed to be the same object
/// on every subsequent call to a component's render-function.
/// ```
/// use bloom_core::use_ref;
/// use std::sync::Mutex;
/// let value = use_ref::<Mutex<usize>>();
/// *value.lock().unwrap() = 42;
/// assert_eq!(*value.lock().unwrap(), 42);
/// ```
pub fn use_ref<T>() -> Arc<T>
where
    T: Default + MaybeSend + MaybeSync + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Arc::new(T::default()).into());
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

pub fn use_ref_with_default<T, D>(default: D) -> Arc<T>
where
    T: MaybeSend + MaybeSync + 'static,
    D: FnOnce() -> T,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let object_ref = hook
                .refs
                .entry(hook.ref_index)
                .or_insert_with(|| Arc::new(default()).into());
            hook.ref_index += 1;
            object_ref
                .clone()
                .downcast()
                .expect("Hook Invariant Violation: Failed to cast ref")
        } else {
            Arc::new(default())
        }
    })
}
