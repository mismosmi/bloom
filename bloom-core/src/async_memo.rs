use crate::{MaybeSend, MaybeSync};
use std::sync::Arc;

use async_context::with_async_context_mut;

use crate::{Identify, hook::Hook};

pub async fn use_async_memo<A, R, F>(args: A, generate: fn(A) -> F) -> Arc<R>
where
    A: Identify,
    F: Future<Output = Arc<R>>,
    R: MaybeSend + MaybeSync + 'static,
{
    let new_id = args.id();

    if let Some((index, old_id, old_value)) = with_async_context_mut(|hook: Option<&mut Hook>| {
        hook.and_then(|hook| {
            let index = hook.memo_index;
            hook.memo_index += 1;

            hook.memos
                .get(index)
                .map(|(id, value)| (index, id.clone(), value.clone()))
        })
    }) {
        if old_id == new_id {
            let old_value: Arc<R> = old_value
                .clone()
                .downcast()
                .expect("Invalid Hook Call: Type mismatch");

            old_value
        } else {
            let new_value = generate(args).await;
            with_async_context_mut(|hook: Option<&mut Hook>| {
                let hook = hook.expect("Invariant Violation: use_async_memo was called in a component cycle but not awaited in the same component");

                if index < hook.memos.len() {
                    hook.memos[index] = (new_id, new_value.clone().into());
                } else {
                    hook.memos.push((new_id, new_value.clone().into()));
                }
                new_value
            })
        }
    } else {
        generate(args).await
    }
}
