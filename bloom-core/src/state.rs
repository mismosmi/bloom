use std::{any::Any, collections::HashMap, ops::Deref, sync::Arc};

use async_channel::{bounded, Sender};
use async_context::with_async_context_mut;

use crate::hook::Hook;

pub(crate) struct StateUpdate {
    update: Box<
        dyn FnOnce(Option<Arc<dyn Any + Send + Sync>>) -> Arc<dyn Any + Send + Sync>
            + Send
            + 'static,
    >,
    index: u16,
}

impl StateUpdate {
    pub(crate) fn apply(self, state: &mut HashMap<u16, Arc<dyn Any + Send + Sync>>) {
        let this_state = state.get_mut(&self.index).cloned();

        let update = self.update;

        let new_state = update(this_state);

        state.insert(self.index, new_state);
    }
}

pub struct State<T> {
    value: Arc<T>,
    signal: Sender<()>,
    updater: Sender<StateUpdate>,
    index: u16,
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

    pub fn update<C, R>(&self, callback: C)
    where
        R: Into<Arc<T>>,
        C: FnOnce(Arc<T>) -> R + Send + Sync + 'static,
    {
        self.updater
            .try_send(StateUpdate {
                update: Box::new(move |value| {
                    let typed_value = value
                        .map(|value| value.downcast().expect("Invalid state hook"))
                        .unwrap_or_default();
                    callback(typed_value).into()
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
            if let Some(value) = hook.state.get(&index) {
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
                let value = Arc::new(T::default());
                State {
                    value,
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
