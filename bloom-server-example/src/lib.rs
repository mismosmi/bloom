pub mod hydration;
#[cfg(not(target_arch = "wasm32"))]
pub mod bundle;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct TokioSpawner;

#[cfg(not(target_arch = "wasm32"))]
impl futures_util::task::Spawn for TokioSpawner {
    fn spawn_obj(
        &self,
        future: futures_util::task::FutureObj<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        tokio::spawn(future);
        Ok(())
    }
}
