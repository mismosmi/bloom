use std::hash::{DefaultHasher, Hash, Hasher};

use async_context::with_async_context_mut;

use crate::hook::Hook;

pub struct Cleanup(Box<dyn FnOnce()>);

impl From<()> for Cleanup {
    fn from(_: ()) -> Self {
        Self(Box::new(|| {}))
    }
}

impl<C> From<C> for Cleanup
where
    C: FnOnce() + 'static,
{
    fn from(cleanup: C) -> Self {
        Self(Box::new(cleanup))
    }
}

impl Cleanup {
    pub(crate) fn run(self) {
        let cleanup = self.0;
        cleanup()
    }
}

pub(crate) struct Effect(Box<dyn FnOnce() -> Cleanup + Send + Sync + 'static>);

impl Effect {
    pub(crate) fn run(self) -> Cleanup {
        let effect = self.0;
        effect()
    }
}

/// Analog to react's useEffect API.
/// The effect runs after the render pass that first renders the component and again whenever
/// the hash of the argument changes. It can return a cleanup function, which runs before the
/// effect runs again and when the component is removed.
/// Cleanups don't have to be `Send`, so they can hold on to JavaScript objects.
pub fn use_effect<A, C>(arg: A, effect: fn(A) -> C)
where
    A: Hash + Send + Sync + 'static,
    C: Into<Cleanup> + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let mut hasher = DefaultHasher::new();
            arg.hash(&mut hasher);
            let arg_hash = hasher.finish();

            hook.effects
                .push((arg_hash, Effect(Box::new(move || effect(arg).into()))));
        }
    })
}
