use std::{any::Any, ops::Deref, sync::Arc};

use async_channel::{bounded, Sender};
use async_context::with_async_context_mut;

use crate::hook::Hook;

pub(crate) struct StateUpdate {
    update:
        Box<dyn FnOnce(Arc<dyn Any + Send + Sync>) -> Arc<dyn Any + Send + Sync> + Send + 'static>,
    index: usize,
}

impl StateUpdate {
    pub(crate) fn apply(self, state: &mut Vec<Arc<dyn Any + Send + Sync>>) {
        let this_state = Arc::clone(
            state
                .get_mut(self.index)
                .expect("Failed to retrieve state at index"),
        );

        let update = self.update;

        let new_state = update(Arc::clone(&this_state));

        state[self.index] = new_state;
    }
}

pub struct State<T> {
    value: Arc<T>,
    signal: Sender<()>,
    updater: Sender<StateUpdate>,
    index: usize,
}

impl<T> Deref for State<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref()
    }
}

impl<T> State<T>
where
    T: Default + Send + Sync + 'static,
{
    fn mock() -> Self {
        let (mock_signal, _) = bounded(0);
        let (mock_updater, _) = bounded(0);
        State {
            value: Arc::new(T::default()),
            signal: mock_signal,
            updater: mock_updater,
            index: 0,
        }
    }

    pub fn update<C>(&self, callback: C)
    where
        C: FnOnce(Arc<T>) -> Arc<T> + Send + 'static,
    {
        self.updater
            .try_send(StateUpdate {
                update: Box::new(move |value| {
                    let typed_value = value.downcast().expect("Invalid state hook");
                    callback(typed_value)
                }),
                index: self.index,
            })
            .expect("Failed to send update");
        let _ = self.signal.try_send(());
    }
}

pub fn use_state<T>() -> State<T>
where
    T: Default + Send + Sync + 'static,
{
    with_async_context_mut(|hook: Option<&mut Hook>| {
        if let Some(hook) = hook {
            let signal = hook.signal.clone();
            let updater = hook.updater.clone();
            let index = hook.state_index;
            hook.state_index += 1;
            if let Some(value) = hook.state.get(index) {
                let value: Arc<T> = value
                    .clone()
                    .downcast()
                    .expect("Invalid Hook Call: Type mismatch");
                State {
                    value,
                    signal,
                    updater,
                    index,
                }
            } else {
                State {
                    value: Arc::new(T::default()),
                    signal,
                    updater,
                    index,
                }
            }
        } else {
            State::mock()
        }
    })
}
