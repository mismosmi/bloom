use async_context::with_async_context_mut;
use std::sync::Arc;

use crate::{hook::Hook, identify::Identify};

/// Memoizes a generated value across renders, returning it in an [`Arc`].
/// Calls `generate` on the first render and whenever the identity of `args` changes.
/// Otherwise, returns a clone of the cached `Arc` without calling `generate`.
/// Call hooks in the same order on every render, and include all inputs to the
/// computation in `args` (use a tuple for multiple inputs).
/// Outside a component's hook context, computes a fresh value on every call.
///
/// ```
/// use bloom_core::use_memo;
///
/// let repeated = use_memo(("hello", 3), |(text, count)| text.repeat(count));
/// assert_eq!(&*repeated, "hellohellohello");
/// ```
pub fn use_memo<A, V>(args: A, generate: fn(A) -> V) -> Arc<V>
where
    A: Identify,
    V: Send + Sync + 'static,
{
    let new_id = args.id();

    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let index = hook.memo_index;
            hook.memo_index += 1;

            if let Some((old_id, value)) = hook.memos.get(index) {
                if old_id == &new_id {
                    let value: Arc<V> = value
                        .clone()
                        .downcast()
                        .expect("Invalid Hook Call: Type mismatch");

                    return value;
                }
            }

            let value = Arc::new(generate(args));
            if index < hook.memos.len() {
                hook.memos[index] = (new_id, value.clone());
            } else {
                hook.memos.push((new_id, value.clone()));
            }
            value
        } else {
            Arc::new(generate(args))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_context::provide_async_context;

    #[tokio::test]
    async fn caches_each_hook_and_recomputes_when_arguments_change() {
        let hook = Hook::from_context(Arc::new(Default::default()), crate::Spawner::mock());
        let ((first, second), mut hook) = provide_async_context(hook, async {
            (use_memo(2, |n| n * 10), use_memo(2, |n| n * 100))
        })
        .await;
        assert_eq!(*first, 20);
        assert_eq!(*second, 200);

        hook.memo_index = 0;
        let ((cached, changed), mut hook) = provide_async_context(hook, async {
            (
                use_memo(2, |_| -> i32 { panic!("cached value must be reused") }),
                use_memo(3, |n| n * 100),
            )
        })
        .await;
        assert!(Arc::ptr_eq(&first, &cached));
        assert_eq!(*changed, 300);
        assert!(!Arc::ptr_eq(&second, &changed));

        hook.memo_index = 0;
        let (_, _) = provide_async_context(hook, async {
            let cached = use_memo(2, |_| -> i32 { panic!("cached value must be reused") });
            let updated = use_memo(3, |_| -> i32 { panic!("updated value must be cached") });
            assert!(Arc::ptr_eq(&first, &cached));
            assert!(Arc::ptr_eq(&changed, &updated));
        })
        .await;
    }

    #[test]
    fn computes_fresh_values_without_hook_context() {
        let first = use_memo(2, |n| n * 10);
        let second = use_memo(2, |n| n * 10);

        assert_eq!(*first, 20);
        assert_eq!(*second, 20);
        assert!(!Arc::ptr_eq(&first, &second));
    }
}
