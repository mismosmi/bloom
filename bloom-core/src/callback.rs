use crate::{Identify, use_memo};
use std::sync::Arc;

struct Callback<C, A, R>
where
    C: Identify,
{
    captures: C,
    cb: fn(&C, A) -> R,
}

impl<C, A, R> Identify for Callback<C, A, R>
where
    C: Identify,
{
    fn id(&self) -> u64 {
        (self.captures.id(), self.cb as usize).id()
    }
}

/// Memoizes a callback, reusing its `Arc` while the captures' identity and
/// callback function remain unchanged. Include every dependency in `captures`.
/// Call hooks in the same order on every render.
/// Outside a component's hook context, returns a fresh callback on every call.
///
/// ```
/// use bloom_core::use_callback;
///
/// let greet = use_callback(String::from("Hello"), |prefix, name: &str| {
///     format!("{prefix}, {name}!")
/// });
/// assert_eq!(greet("Bloom"), "Hello, Bloom!");
/// ```
pub fn use_callback<C, A, R>(
    captures: C,
    cb: fn(&C, A) -> R,
) -> Arc<impl Fn(A) -> R + Send + Sync + 'static>
where
    C: Identify + Send + Sync + 'static,
    A: 'static,
    R: 'static,
{
    use_memo(Callback { captures, cb }, |Callback { captures, cb }| {
        Arc::new(move |args| cb(&captures, args))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::Hook;
    use async_context::provide_async_context;

    // Exercise custom identity without requiring Hash on captures.
    struct Captures(i32);

    impl Identify for Captures {
        fn id(&self) -> u64 {
            self.0.id()
        }
    }

    fn add(captures: &Captures, value: i32) -> i32 {
        captures.0 + value
    }

    fn multiply(captures: &Captures, value: i32) -> i32 {
        captures.0 * value
    }

    #[tokio::test]
    async fn caches_callback_and_updates_captures_and_function() {
        let hook = Hook::from_context(Arc::new(Default::default()), crate::Spawner::mock());
        let (first, mut hook) =
            provide_async_context(hook, async { use_callback(Captures(2), add) }).await;
        assert_eq!(first(3), 5);

        hook.memo_index = 0;
        let (cached, mut hook) =
            provide_async_context(hook, async { use_callback(Captures(2), add) }).await;
        assert!(Arc::ptr_eq(&first, &cached));

        hook.memo_index = 0;
        let (changed, mut hook) =
            provide_async_context(hook, async { use_callback(Captures(4), add) }).await;
        assert!(!Arc::ptr_eq(&cached, &changed));
        assert_eq!(changed(3), 7);
        assert_eq!(first(3), 5);

        hook.memo_index = 0;
        let (switched, _) =
            provide_async_context(hook, async { use_callback(Captures(4), multiply) }).await;
        assert!(!Arc::ptr_eq(&changed, &switched));
        assert_eq!(switched(3), 12);
    }

    #[test]
    fn owns_captures_and_returns_fresh_callbacks_without_context() {
        fn append(prefix: &String, suffix: &str) -> String {
            format!("{prefix}{suffix}")
        }

        let first = use_callback(String::from("hello "), append);
        let second = use_callback(String::from("hello "), append);
        assert_eq!(first("world"), "hello world");
        assert!(!Arc::ptr_eq(&first, &second));
    }
}
