use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use async_channel::{Sender, bounded};

use crate::Spawner;
use crate::effect::Effect;
use crate::state::StateUpdate;

pub(crate) struct Hook {
    pub(crate) signal: Sender<()>,
    pub(crate) updater: Sender<StateUpdate>,
    pub(crate) state: HashMap<u16, crate::platform::SharedValue>,
    pub(crate) state_index: u16,
    pub(crate) effects: Vec<(u64, Effect)>,
    pub(crate) refs: HashMap<u16, crate::platform::SharedValue>,
    pub(crate) ref_index: u16,
    pub(crate) context: Arc<HashMap<TypeId, crate::platform::SharedValue>>,
    pub(crate) memos: Vec<(u64, crate::platform::SharedValue)>,
    pub(crate) memo_index: usize,
    pub(crate) spawner: Spawner,
}

impl Hook {
    pub(crate) fn new(
        signal: Sender<()>,
        updater: Sender<StateUpdate>,
        state: HashMap<u16, crate::platform::SharedValue>,
        refs: HashMap<u16, crate::platform::SharedValue>,
        memos: Vec<(u64, crate::platform::SharedValue)>,
        context: Arc<HashMap<TypeId, crate::platform::SharedValue>>,
        spawner: Spawner,
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
            memos,
            memo_index: 0,
            spawner,
        }
    }

    pub(crate) fn from_context(
        context: Arc<HashMap<TypeId, crate::platform::SharedValue>>,
        spawner: Spawner,
    ) -> Self {
        let (signal, _) = bounded(1);
        let (updater, _) = bounded(1);

        Self {
            signal,
            updater,
            state: HashMap::new(),
            state_index: 0,
            effects: Vec::new(),
            refs: HashMap::new(),
            ref_index: 0,
            context,
            memos: Vec::new(),
            memo_index: 0,
            spawner,
        }
    }
}
