use futures_util::task::{FutureObj, Spawn, SpawnError};

/// Runs async components on the current tokio runtime.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSpawner;

impl Spawn for TokioSpawner {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::spawn(future);
        Ok(())
    }
}
