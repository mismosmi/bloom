use bloom_core::{MaybeSpawn, platform::TaskFuture};

#[derive(Clone)]
pub(crate) struct WasmSpawner;

impl MaybeSpawn for WasmSpawner {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn spawn_obj(
        &self,
        future: TaskFuture<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        wasm_bindgen_futures::spawn_local(future);
        Ok(())
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn spawn_local_obj(
        &self,
        future: TaskFuture<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        wasm_bindgen_futures::spawn_local(future);
        Ok(())
    }
}
