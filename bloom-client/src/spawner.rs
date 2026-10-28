use futures_util::task::Spawn;

#[derive(Clone)]
pub(crate) struct WasmSpawner;

impl Spawn for WasmSpawner {
    fn spawn_obj(
        &self,
        future: futures_util::task::FutureObj<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        wasm_bindgen_futures::spawn_local(future);
        Ok(())
    }
}
