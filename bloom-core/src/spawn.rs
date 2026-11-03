use async_context::with_async_context;
use futures_executor::block_on;
use futures_util::task::{FutureObj, Spawn, SpawnError};

use crate::hook::Hook;

trait CloneSpawn: Spawn + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneSpawn>;
}

impl<S: Spawn + Clone + Send + Sync + 'static> CloneSpawn for S {
    fn clone_box(&self) -> Box<dyn CloneSpawn> {
        Box::new(self.clone())
    }
}

/// A type-erased spawner that clones the underlying spawner when cloned.
pub struct Spawner(Box<dyn CloneSpawn>);

#[derive(Clone)]
struct BlockingSpawner;

impl Spawn for BlockingSpawner {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        block_on(future);
        Ok(())
    }
}

impl Spawner {
    /// Wraps a cloneable spawner.
    pub fn new(spawner: impl Spawn + Clone + Send + Sync + 'static) -> Self {
        Self(Box::new(spawner))
    }

    pub fn mock() -> Self {
        Self(Box::new(BlockingSpawner))
    }
}

impl Clone for Spawner {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl Spawn for Spawner {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        self.0.spawn_obj(future)
    }

    fn status(&self) -> Result<(), SpawnError> {
        self.0.status()
    }
}

pub fn use_spawn() -> Spawner {
    with_async_context(|hook: Option<&Hook>| {
        if let Some(hook) = hook {
            hook.spawner.clone()
        } else {
            Spawner::mock()
        }
    })
}
