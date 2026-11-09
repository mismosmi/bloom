use async_context::with_async_context;
use futures_executor::block_on;
use futures_util::{future::RemoteHandle, task::SpawnError};

use crate::{Identify, MaybeSend, MaybeSpawn, MaybeSync, hook::Hook, platform::TaskFuture};

trait CloneSpawn: MaybeSpawn + MaybeSend + MaybeSync {
    fn clone_box(&self) -> Box<dyn CloneSpawn>;
}
impl<S: MaybeSpawn + Clone + MaybeSend + MaybeSync + 'static> CloneSpawn for S {
    fn clone_box(&self) -> Box<dyn CloneSpawn> {
        Box::new(self.clone())
    }
}

/// A type-erased spawner that clones the underlying spawner when cloned.
/// Uses `LocalSpawn` on browser WebAssembly and `Spawn` on other targets.
pub struct Spawner(Box<dyn CloneSpawn>);

#[derive(Clone)]
struct BlockingSpawner;
impl MaybeSpawn for BlockingSpawner {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn spawn_obj(&self, future: TaskFuture<'static, ()>) -> Result<(), SpawnError> {
        block_on(future);
        Ok(())
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn spawn_local_obj(&self, future: TaskFuture<'static, ()>) -> Result<(), SpawnError> {
        block_on(future);
        Ok(())
    }
}
impl Spawner {
    /// Wraps a cloneable spawner.
    pub fn new(spawner: impl MaybeSpawn + Clone + MaybeSend + MaybeSync + 'static) -> Self {
        Self(Box::new(spawner))
    }
    pub fn mock() -> Self {
        Self(Box::new(BlockingSpawner))
    }

    /// Spawns a future using the target's thread-safety requirements.
    pub fn spawn(
        &self,
        future: impl Future<Output = ()> + MaybeSend + 'static,
    ) -> Result<(), SpawnError> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            futures_util::task::SpawnExt::spawn(self, future)
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            futures_util::task::LocalSpawnExt::spawn_local(self, future)
        }
    }
    /// Spawns a future and returns a handle to its result.
    pub fn spawn_with_handle<F>(&self, future: F) -> Result<RemoteHandle<F::Output>, SpawnError>
    where
        F: Future + MaybeSend + 'static,
        F::Output: MaybeSend + 'static,
    {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            futures_util::task::SpawnExt::spawn_with_handle(self, future)
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            futures_util::task::LocalSpawnExt::spawn_local_with_handle(self, future)
        }
    }
}
impl Clone for Spawner {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}
impl MaybeSpawn for Spawner {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn spawn_obj(&self, future: TaskFuture<'static, ()>) -> Result<(), SpawnError> {
        self.0.spawn_obj(future)
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn spawn_local_obj(&self, future: TaskFuture<'static, ()>) -> Result<(), SpawnError> {
        self.0.spawn_local_obj(future)
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn status(&self) -> Result<(), SpawnError> {
        self.0.status()
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn status_local(&self) -> Result<(), SpawnError> {
        self.0.status_local()
    }
}
pub fn use_spawner() -> Spawner {
    with_async_context(|hook: Option<&Hook>| {
        hook.map(|hook| hook.spawner.clone())
            .unwrap_or_else(Spawner::mock)
    })
}
impl Identify for Spawner {
    fn id(&self) -> u64 {
        Box::as_ptr(&self.0).addr() as u64
    }
}
