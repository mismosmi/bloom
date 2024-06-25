#![cfg(test)]

use futures_util::task::Spawn;

#[derive(Clone)]
pub(crate) struct TokioSpawner;

impl Spawn for TokioSpawner {
    fn spawn_obj(
        &self,
        future: futures_util::task::FutureObj<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        tokio::spawn(future);
        Ok(())
    }
}
