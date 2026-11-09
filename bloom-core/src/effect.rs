use crate::{MaybeSend, MaybeSync};
use async_context::with_async_context_mut;

use crate::{hook::Hook, identify::Identify};

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

trait EffectFn: FnOnce() -> Cleanup + MaybeSend + MaybeSync {}
impl<T: FnOnce() -> Cleanup + MaybeSend + MaybeSync> EffectFn for T {}
pub(crate) struct Effect(Box<dyn EffectFn>);

impl Effect {
    pub(crate) fn run(self) -> Cleanup {
        let effect = self.0;
        effect()
    }
}

/// Analog to react's useEffect API.
/// The effect runs after the render pass that first renders the component and again whenever
/// the identity of the argument changes. It can return a cleanup function, which runs before the
/// effect runs again and when the component is removed.
/// Cleanups don't have to be `Send`, so they can hold on to JavaScript objects.
pub fn use_effect<A, C>(arg: A, effect: fn(A) -> C)
where
    A: Identify + MaybeSend + MaybeSync + 'static,
    C: Into<Cleanup> + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let arg_id = arg.id();

            hook.effects
                .push((arg_id, Effect(Box::new(move || effect(arg).into()))));
        }
    })
}
