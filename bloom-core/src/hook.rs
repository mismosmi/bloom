use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use async_channel::{bounded, Sender};

use crate::effect::Effect;
use crate::state::StateUpdate;

pub(crate) struct Hook {
    pub(crate) signal: Sender<()>,
    pub(crate) updater: Sender<StateUpdate>,
    pub(crate) state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
    pub(crate) state_index: u16,
    pub(crate) effects: Vec<(u64, Effect)>,
    pub(crate) refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
    pub(crate) ref_index: u16,
    pub(crate) context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl Hook {
    pub(crate) fn new(
        signal: Sender<()>,
        updater: Sender<StateUpdate>,
        state: HashMap<u16, Arc<dyn Any + Send + Sync>>,
        refs: HashMap<u16, Arc<dyn Any + Send + Sync + 'static>>,
        context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    ) -> Self {
        Self {
            updater,
            state,
            signal,
            state_index: 0,
            effects: Vec::new(),
            refs,
            ref_index: 0,
            context,
        }
    }

    pub(crate) fn from_context(context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>) -> Self {
        let (signal, _) = bounded(0);
        let (updater, _) = bounded(0);

        Self {
            signal,
            updater,
            state: HashMap::new(),
            state_index: 0,
            effects: Vec::new(),
            refs: HashMap::new(),
            ref_index: 0,
            context,
        }
    }
}
