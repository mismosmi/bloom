use async_context::with_async_context_mut;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use crate::hook::Hook;

pub fn use_memo<A, V>(args: A, generate: fn(A) -> V) -> Arc<V>
where
    A: Hash,
    V: Send + Sync + 'static,
{
    let mut hasher = DefaultHasher::new();
    args.hash(&mut hasher);
    let new_hash = hasher.finish();

    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let index = hook.memo_index;
            hook.memo_index += 1;

            if let Some((old_hash, value)) = hook.memos.get(index) {
                if old_hash == &new_hash {
                    let value: Arc<V> = value
                        .clone()
                        .downcast()
                        .expect("Invalid Hook Call: Type mismatch");

                    return value;
                }
            }

            let value = Arc::new(generate(args));
            hook.memos[index] = (new_hash, value.clone());
            value
        } else {
            Arc::new(generate(args))
        }
    })
}
