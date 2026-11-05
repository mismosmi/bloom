use std::{any::Any, collections::HashMap, hash::Hash, ops::Deref, sync::Arc};

use async_channel::{Sender, bounded};
use async_context::with_async_context_mut;

use crate::{Element, Identify, hook::Hook};

type StateUpdateFn = Box<
    dyn FnOnce(Option<Arc<dyn Any + Send + Sync>>) -> Arc<dyn Any + Send + Sync> + Send + 'static,
>;

pub(crate) struct StateUpdate {
    update: StateUpdateFn,
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
/// use bloom_core::use_state;
///
/// let my_state = use_state(|| 0.into());
///
/// assert_eq!(0, *my_state);
/// ```
///
/// Its update method changes the state and triggers a re-render of the component.
/// Call it from a callback, such as an event handler:
/// ```
/// use bloom_core::{component, use_state, Element};
/// use std::sync::Arc;
/// #[component]
/// fn Counter() -> Result<Element<String, ()>, ()> {
///     let count = use_state(|| Arc::new(0));
///     let increment = move || count.update(|value| (*value + 1).into());
///     // Pass `increment` to the renderer's event handler.
///     Ok("Increment".into())
/// }
/// ```
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
    fn mock<I>(value: I) -> Self
    where
        I: Into<Arc<T>>,
    {
        let (mock_signal, _) = bounded(1);
        let (mock_updater, _) = bounded(1);
        State {
            value: value.into(),
            signal: mock_signal,
            updater: mock_updater,
            index: 0,
        }
    }

    pub fn update<C>(&self, callback: C)
    where
        C: FnOnce(Arc<T>) -> Arc<T> + Send + Sync + 'static,
    {
        let current_value = self.value.clone();
        self.updater
            .try_send(StateUpdate {
                update: Box::new(move |value| {
                    let typed_value = value
                        .map(|value| value.downcast().expect("Invalid state hook"))
                        .unwrap_or(current_value);
                    callback(typed_value)
                }),
                index: self.index,
            })
            .expect("Failed to send update");
        let _ = self.signal.try_send(());
    }

    pub fn set<I>(&self, value: I)
    where
        I: Into<Arc<T>>,
    {
        let typed_value = value.into();
        self.updater
            .try_send(StateUpdate {
                update: Box::new(move |_| typed_value),
                index: self.index,
            })
            .expect("Failed to send new state");
        let _ = self.signal.try_send(());
    }
}

impl<T> Identify for State<T>
where
    T: Identify,
{
    fn id(&self) -> u64 {
        let value: &T = self;
        value.id()
    }
}

impl<T> Into<Arc<T>> for State<T>
where
    T: Identify,
{
    fn into(self) -> Arc<T> {
        self.value
    }
}

/// Analog to react's useState API.
/// Pass a callback to build the initial state.
/// The returned State-object can be used to read and update the state.
pub fn use_state<T, D>(default: D) -> State<T>
where
    T: Send + Sync + 'static,
    D: FnOnce() -> Arc<T>,
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
                // Store the initial value so later renders don't build it again
                let value = default();
                hook.state.insert(index, value.clone());
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
