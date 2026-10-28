use std::sync::Arc;

use futures_util::task::{Spawn, SpawnExt};

use crate::{
    commit::Changes,
    hook::Hook,
    render_context::RenderContext,
    suspense::{run_or_suspend, RunOrSuspendResult},
};

use super::{
    tree::{TreeComponent, TreeNode},
    visit::visit,
    ComponentWork, Pass, RenderOutput, Work,
};

impl<N, E> TreeComponent<N, E>
where
    N: From<String>,
{
    /// Whether the component has to render in this render pass
    pub(super) fn needs_render(&mut self, work: ComponentWork<N, E>) -> bool {
        match work {
            ComponentWork::Create => true,
            ComponentWork::NewProps(component) => {
                self.component = component;
                true
            }
            // A suspended render that is still waiting is replaced if the state changed
            ComponentWork::Reload => {
                !self.updates.is_empty() || (self.render_result.is_none() && self.child.is_none())
            }
        }
    }

    /// Picks up a suspended render that has finished and reloads the child
    pub(super) async fn reload<S>(
        &mut self,
        ctx: RenderContext<N>,
        pass: &Pass<S>,
    ) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        if let Some(render_result) = self.render_result.take() {
            match run_or_suspend(render_result) {
                RunOrSuspendResult::Suspend(render_result) => {
                    self.render_result = Some(render_result)
                }
                RunOrSuspendResult::Done(output) => {
                    return self.apply_render(output, ctx, pass).await
                }
            }
        }
        match self.child.as_mut() {
            Some(child) => visit(child, Work::Reload, ctx, pass).await,
            None => Ok(Changes::default()),
        }
    }

    /// Renders the component with its pending state updates applied.
    /// A render that does not finish on its first poll suspends and keeps running on the spawner,
    /// its result is picked up by a later render pass.
    pub(super) async fn render<S>(
        &mut self,
        ctx: RenderContext<N>,
        pass: &Pass<S>,
    ) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        while let Ok(state_update) = self.updates.try_recv() {
            state_update.apply(&mut self.state);
        }

        // A newer render replaces one that suspended before
        self.render_result = None;
        let hook = Hook::new(
            pass.signal.clone(),
            self.updater.clone(),
            self.state.clone(),
            self.refs.clone(),
            ctx.context.clone(),
        );
        let component = Arc::clone(&self.component);
        match run_or_suspend(Box::pin(async_context::provide_async_context(
            hook,
            component.render(),
        ))) {
            RunOrSuspendResult::Done(output) => self.apply_render(output, ctx, pass).await,
            RunOrSuspendResult::Suspend(render_future) => {
                let signal = pass.signal.clone();
                self.render_result = Some(Box::pin(
                    pass.spawner
                        .spawn_with_handle(async move {
                            let output = render_future.await;
                            let _ = signal.try_send(());
                            output
                        })
                        .expect("Failed to spawn async task"),
                ));
                Ok(Changes::default())
            }
        }
    }

    /// Queues the effects of a finished render and diffs its output
    pub(super) async fn apply_render<S>(
        &mut self,
        (element, hook): RenderOutput<N, E>,
        ctx: RenderContext<N>,
        pass: &Pass<S>,
    ) -> Result<Changes<N>, E>
    where
        N: Send + Sync + 'static,
        E: Send + 'static,
        S: Spawn + Send + Sync + 'static,
    {
        let mut changes = Changes::default();
        // Effects of a detached component only run once its nodes are shown
        if ctx.detached {
            self.deferred_effects = Some(hook.effects);
        } else {
            changes.queue_effects(self.id, hook.effects);
        }
        self.state = hook.state;
        self.refs = hook.refs;
        let element = element?;
        match self.child.as_mut() {
            Some(child) => changes.append(visit(child, Work::Update(element), ctx, pass).await?),
            None => {
                let mut child = Box::new(TreeNode::from(element));
                changes.append(visit(&mut child, Work::Create, ctx, pass).await?);
                self.child = Some(child);
            }
        }
        Ok(changes)
    }
}
