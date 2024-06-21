use std::any::Any;
use std::sync::Arc;

use async_channel::Sender;

use crate::state::StateUpdate;

pub(crate) struct Hook {
    pub(crate) signal: Sender<()>,
    pub(crate) updater: Sender<StateUpdate>,
    pub(crate) state: Vec<Arc<dyn Any + Send + Sync>>,
    pub(crate) state_index: usize,
}

impl Hook {
    pub(crate) fn new(
        signal: Sender<()>,
        updater: Sender<StateUpdate>,
        state: Vec<Arc<dyn Any + Send + Sync>>,
    ) -> Self {
        Self {
            updater,
            state,
            signal,
            state_index: 0,
        }
    }
}
