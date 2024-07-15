use std::{any::Any, collections::HashMap, hash::Hash, ops::Deref, sync::Arc};

use async_channel::{bounded, Sender};
use async_context::with_async_context_mut;

use crate::{hook::Hook, Element};

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

/// The state object can be dereferenced to obtain the current value.
/// ```
/// let my_state = use_state(|| 0);
///
/// assert_eq!(0, *my_state);
/// ```
///
/// It's update-method can be used to change the state.
/// ```
/// my_state.update(|value| *value + 1);
/// ```
/// This will trigger a re-render of the component.
#[derive(Clone)]
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

impl<T> Hash for State<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.value).hash(state);
        self.index.hash(state);
    }
}

impl<N, E, T> From<State<T>> for Element<N, E>
where
    N: From<String>,
    T: ToString,
{
    fn from(value: State<T>) -> Self {
        let value: &T = &value;
        Element::Node(N::from(value.to_string()), Vec::new())
    }
}

impl<T> State<T>
where
    T: Send + Sync + 'static,
{
    fn mock(value: T) -> Self {
        let (mock_signal, _) = bounded(0);
        let (mock_updater, _) = bounded(0);
        State {
            value: Arc::new(value),
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
        let current_value = self.value.clone();
        self.updater
            .try_send(StateUpdate {
                update: Box::new(move |value| {
                    let typed_value = value
                        .map(|value| value.downcast().expect("Invalid state hook"))
                        .unwrap_or(current_value);
                    callback(typed_value).into()
                }),
                index: self.index,
            })
            .expect("Failed to send update");
        let _ = self.signal.try_send(());
    }
}

/// Analog to react's useState API.
/// Pass a callback to build the initial state.
/// The returned State-object can be used to read and update the state.
pub fn use_state<T, D>(default: D) -> State<T>
where
    T: Send + Sync + 'static,
    D: FnOnce() -> T,
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
                let value = Arc::new(default());
                State {
                    value,
                    signal,
                    updater,
                    index,
                }
            }
        } else {
            State::mock(default())
        }
    })
}
