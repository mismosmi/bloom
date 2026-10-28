use std::{
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::{
    future,
    task::{Spawn, SpawnExt},
    Future,
};

use crate::{
    commit::Changes,
    component::ComponentDiff,
    error_boundary::{Dismiss, Fallback},
    render_context::RenderContext,
    CaughtError, Element,
};

use super::{
    chain,
    children::update_children,
    tree::{element_key, SuspenseState, TreeErrorBoundary, TreeNode, TreeSuspense},
    ComponentWork, Failure, Pass, Visit, Visited, Work, TASK_DEPTH,
};

/// Diffs a subtree and collects its changes in document order.
/// Components that render are visited in their own task on the spawner, so siblings are rendered
/// and diffed in parallel. Everything else is visited in place and continues in a new task
/// every `TASK_DEPTH` levels, so deeply nested trees fit on the stack.
pub(super) fn visit<'a, N, E, S>(
    node: &'a mut TreeNode<N, E>,
    work: Work<N, E>,
    ctx: RenderContext<N>,
    pass: &'a Pass<S>,
) -> Visit<'a, N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    Box::pin(async move {
        let mut ctx = ctx;
        // Deeply nested nodes continue in a new task so they fit on the stack
        if ctx.depth >= TASK_DEPTH {
            return visit_in_task(node, work, ctx, pass).await;
        }
        ctx.depth += 1;
        match work {
            Work::Create => create(node, ctx, pass).await,
            Work::Reload => reload(node, ctx, pass).await,
            Work::Update(next) => update(node, next, ctx, pass).await,
        }
    })
}

/// Moves a subtree into a task on the spawner and puts it back once it is visited
pub(super) async fn visit_in_task<N, E, S>(
    node: &mut TreeNode<N, E>,
    work: Work<N, E>,
    mut ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let mut subtree = mem::replace(node, TreeNode::Fragment(Vec::new()));
    let task_pass = pass.clone();
    ctx.depth = 0;
    let task = pass
        .spawner
        .spawn_with_handle(async move {
            let result = visit(&mut subtree, work, ctx, &task_pass).await;
            (subtree, result)
        })
        .expect("Failed to spawn async task");
    let (subtree, result) = task.await;
    *node = subtree;
    result
}

/// Visits the subtrees concurrently and joins their changes in order.
/// Every subtree is visited to the end even if another one fails, so none of them is lost
/// halfway, e.g. while its component is moved into a task.
/// Only the first error is kept.
pub(super) async fn visit_all<N, E>(visits: Vec<Visit<'_, N, E>>) -> Visited<N, E> {
    let mut changes = Changes::default();
    let mut error = None;
    for visited in future::join_all(visits).await {
        match visited {
            Ok(child_changes) => changes.append(child_changes),
            Err(failure) => {
                changes.append(failure.changes);
                error.get_or_insert(failure.error);
            }
        }
    }
    match error {
        Some(error) => Err(Failure { error, changes }),
        None => Ok(changes),
    }
}

/// A component that renders is moved into a task on the spawner, so sibling components render
/// in parallel, and put back once it is visited. Other components are visited in place.
pub(super) async fn visit_component<N, E, S>(
    node: &mut TreeNode<N, E>,
    work: ComponentWork<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let TreeNode::Component(component) = node else {
        unreachable!("Only components are visited as components")
    };
    if !component.needs_render(work) {
        return component.reload(ctx, pass).await;
    }

    let TreeNode::Component(mut component) = mem::replace(node, TreeNode::Fragment(Vec::new()))
    else {
        unreachable!("Only components are visited as components")
    };
    let task_pass = pass.clone();
    let mut ctx = ctx;
    ctx.depth = 0;
    let task = pass
        .spawner
        .spawn_with_handle(async move {
            let result = component.render(ctx, &task_pass).await;
            (component, result)
        })
        .expect("Failed to spawn async task");
    let (component, result) = task.await;
    *node = TreeNode::Component(component);
    result
}

pub(super) async fn create<N, E, S>(
    node: &mut TreeNode<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let mut changes = Changes::default();
    match node {
        TreeNode::Component(_) => {
            return visit_component(node, ComponentWork::Create, ctx, pass).await;
        }
        TreeNode::Node(node, children) => {
            if !ctx.detached {
                changes.create_node(node, &ctx.parent, &ctx.sibling);
            }
            changes.merge(create_children(children, ctx.with_parent(node.clone()), pass).await)?;
        }
        // Children are created in order, so each one is inserted before the
        // fragment's sibling, which already exists, rather than before the next child
        TreeNode::Fragment(children) => {
            changes.merge(create_children(children, ctx, pass).await)?;
        }
        TreeNode::Provider(value, children) => {
            let children_ctx = ctx.with_context(value.clone());
            changes.merge(create_children(children, children_ctx, pass).await)?;
        }
        TreeNode::Keyed(_, child) => return visit(child, Work::Create, ctx, pass).await,
        // The fallback is only created once it is clear that the children suspend
        TreeNode::Suspense(suspense) => {
            changes.merge(create_children(&mut suspense.children, ctx.detach(), pass).await)?;
            changes.merge(settle(suspense, ctx, pass).await)?;
        }
        TreeNode::ErrorBoundary(boundary) => {
            let visited = create_children(&mut boundary.children, ctx.clone(), pass).await;
            return catch(boundary, visited, ctx, pass).await;
        }
    }
    Ok(changes)
}

/// Creates children that share the parent and sibling of the context
fn create_children<'a, N, E, S>(
    children: &'a mut [TreeNode<N, E>],
    ctx: RenderContext<N>,
    pass: &'a Pass<S>,
) -> impl Future<Output = Visited<N, E>> + Send + 'a
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    visit_all(
        children
            .iter_mut()
            .map(|child| visit(child, Work::Create, ctx.clone(), pass))
            .collect(),
    )
}

/// Reloads children that share the parent and sibling of the context
fn reload_children<'a, N, E, S>(
    children: &'a mut [TreeNode<N, E>],
    ctx: &RenderContext<N>,
    pass: &'a Pass<S>,
) -> impl Future<Output = Visited<N, E>> + Send + 'a
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let anchors = sibling_anchors(children, ctx.sibling.clone());
    visit_all(
        children
            .iter_mut()
            .zip(anchors)
            .map(|(child, sibling)| visit(child, Work::Reload, ctx.with_sibling(sibling), pass))
            .collect(),
    )
}

/// The sibling of each child is the first node of the child after it before this render pass
pub(super) fn sibling_anchors<N, E>(
    children: &[TreeNode<N, E>],
    last: Option<Arc<N>>,
) -> Vec<Option<Arc<N>>>
where
    N: From<String>,
{
    let mut sibling = last;
    let mut anchors = Vec::with_capacity(children.len());
    for child in children.iter().rev() {
        anchors.push(sibling);
        sibling = child.get_first_node();
    }
    anchors.reverse();
    anchors
}

pub(super) async fn reload<N, E, S>(
    node: &mut TreeNode<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    match node {
        TreeNode::Component(_) => visit_component(node, ComponentWork::Reload, ctx, pass).await,
        TreeNode::Node(node, children) => {
            let anchors = sibling_anchors(children, None);
            visit_all(
                children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        let child_ctx = ctx.with_parent_and_sibling(node.clone(), sibling);
                        visit(child, Work::Reload, child_ctx, pass)
                    })
                    .collect(),
            )
            .await
        }
        TreeNode::Fragment(children) => reload_children(children, &ctx, pass).await,
        TreeNode::Provider(value, children) => {
            let anchors = sibling_anchors(children, ctx.sibling.clone());
            visit_all(
                children
                    .iter_mut()
                    .zip(anchors)
                    .map(|(child, sibling)| {
                        let child_ctx = ctx.with_sibling_and_context(sibling, value.clone());
                        visit(child, Work::Reload, child_ctx, pass)
                    })
                    .collect(),
            )
            .await
        }
        TreeNode::Keyed(_, child) => visit(child, Work::Reload, ctx, pass).await,
        TreeNode::Suspense(suspense) => {
            let children_ctx = suspense.children_ctx(&ctx);
            let mut changes = reload_children(&mut suspense.children, &children_ctx, pass).await?;
            if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                changes.merge(visit(fallback, Work::Reload, ctx.clone(), pass).await)?;
            }
            changes.merge(settle(suspense, ctx, pass).await)?;
            Ok(changes)
        }
        TreeNode::ErrorBoundary(boundary) => {
            let visited = reload_children(&mut boundary.children, &ctx, pass).await;
            catch(boundary, visited, ctx, pass).await
        }
    }
}

pub(super) async fn update<N, E, S>(
    node: &mut TreeNode<N, E>,
    next: Element<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    // A changed key means a different element, so it is replaced instead of
    // updated in place. Removing the old element runs its cleanups.
    if node.key() != element_key(&next) {
        return replace_node(node, next, ctx, pass).await;
    }
    if let (TreeNode::Component(current), Element::Component(next_component)) = (&*node, &next) {
        let work = match next_component.compare(current.component.as_any()) {
            ComponentDiff::Equal => ComponentWork::Reload,
            ComponentDiff::NewProps => ComponentWork::NewProps(Arc::clone(next_component)),
            // Removing the old component runs its cleanups
            ComponentDiff::NewType => return replace_node(node, next, ctx, pass).await,
        };
        return visit_component(node, work, ctx, pass).await;
    }

    let mut changes = Changes::default();
    match (node, next) {
        (TreeNode::Node(current, current_children), Element::Node(next, next_children)) => {
            let next = Arc::new(next);
            if !ctx.detached {
                changes.update_node(current, &next);
            }
            *current = next.clone();
            changes.merge(
                update_children(current_children, next_children, ctx.with_parent(next), pass).await,
            )?;
        }
        (TreeNode::Fragment(current_children), Element::Fragment(next_children)) => {
            changes.merge(update_children(current_children, next_children, ctx, pass).await)?
        }
        (TreeNode::Provider(_, current_children), Element::Provider(next_value, next_children)) => {
            changes.merge(
                update_children(
                    current_children,
                    next_children,
                    ctx.with_context(next_value),
                    pass,
                )
                .await,
            )?
        }
        // The keys are equal, they were compared above
        (TreeNode::Keyed(_, current_child), Element::Keyed(_, next_element)) => {
            return visit(current_child, Work::Update(*next_element), ctx, pass).await;
        }
        (TreeNode::Suspense(suspense), Element::Suspense(next_fallback, next_children)) => {
            let children_ctx = suspense.children_ctx(&ctx);
            changes.merge(
                update_children(&mut suspense.children, next_children, children_ctx, pass).await,
            )?;
            // Once revealed, the fallback is never shown again
            if let SuspenseState::Fallback(fallback) = &mut suspense.state {
                changes.merge(
                    visit(fallback, Work::Update(*next_fallback), ctx.clone(), pass).await,
                )?;
            }
            changes.merge(settle(suspense, ctx, pass).await)?;
        }
        (TreeNode::ErrorBoundary(boundary), Element::ErrorBoundary(fallback, next_children)) => {
            return update_error_boundary(boundary, fallback, next_children, ctx, pass).await;
        }
        (node, next) => return replace_node(node, next, ctx, pass).await,
    }
    Ok(changes)
}

/// Boxed, so the state of the boundary doesn't add to every level of the recursion in `update`
fn update_error_boundary<'a, N, E, S>(
    boundary: &'a mut TreeErrorBoundary<N, E>,
    fallback: Fallback<N, E>,
    next_children: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &'a Pass<S>,
) -> Visit<'a, N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    Box::pin(async move {
        boundary.fallback = fallback;
        if let Some(dismissed) = &boundary.failed {
            // A boundary that failed keeps its fallback and ignores the next children
            if !dismissed.load(Ordering::SeqCst) {
                return reload_children(&mut boundary.children, &ctx, pass).await;
            }
            // The error was dismissed, so the children are created again in place of the fallback
            boundary.failed = None;
            let fallback = mem::replace(
                &mut boundary.children,
                next_children.into_iter().map(TreeNode::from).collect(),
            );
            let visited = create_children(&mut boundary.children, ctx.clone(), pass).await;
            let mut removed = Changes::default();
            for node in fallback {
                remove(node, ctx.attached_parent(), &mut removed);
            }
            return catch(boundary, chain(visited, removed), ctx, pass).await;
        }
        let visited =
            update_children(&mut boundary.children, next_children, ctx.clone(), pass).await;
        catch(boundary, visited, ctx, pass).await
    })
}

/// Runs after everything below a suspense boundary has been visited
pub(super) async fn settle<N, E, S>(
    suspense: &mut TreeSuspense<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let pending = !suspense.is_revealed() && suspense.children.iter().any(TreeNode::is_pending);
    let mut changes = Changes::default();
    match mem::replace(&mut suspense.state, SuspenseState::Revealed) {
        SuspenseState::Revealed => {}
        SuspenseState::Unsettled(fallback) if pending => {
            let mut fallback = Box::new(TreeNode::from(fallback));
            let visited = visit(&mut fallback, Work::Create, ctx, pass).await;
            // The fallback is kept even if it fails, so its nodes are removed with the boundary
            suspense.state = SuspenseState::Fallback(fallback);
            return visited;
        }
        SuspenseState::Fallback(fallback) if pending => {
            suspense.state = SuspenseState::Fallback(fallback);
        }
        previous => {
            // A boundary inside a detached subtree is mounted along with it
            if !ctx.detached {
                for child in &mut suspense.children {
                    child.mount(&ctx.parent, &ctx.sibling, &mut changes);
                }
            }
            if let SuspenseState::Fallback(fallback) = previous {
                remove(*fallback, ctx.attached_parent(), &mut changes);
            }
        }
    }
    Ok(changes)
}

/// Removes a subtree that is no longer part of the tree.
/// The parent is none if the nodes were never passed to the object model.
pub(super) fn remove<N, E>(node: TreeNode<N, E>, parent: Option<Arc<N>>, changes: &mut Changes<N>)
where
    N: From<String>,
{
    // The subtree is owned, so it is walked with a stack instead of recursion
    let mut stack = vec![(node, parent)];
    while let Some((node, parent)) = stack.pop() {
        match node {
            TreeNode::Component(component) => {
                changes.unmount(component.id);
                if let Some(child) = component.child {
                    stack.push((*child, parent));
                }
            }
            TreeNode::Node(node, children) => {
                if let Some(parent) = &parent {
                    changes.remove_node(&node, parent);
                }
                for child in children {
                    stack.push((child, parent.as_ref().map(|_| Arc::clone(&node))));
                }
            }
            TreeNode::Fragment(children)
            | TreeNode::Provider(_, children)
            | TreeNode::ErrorBoundary(TreeErrorBoundary { children, .. }) => {
                for child in children {
                    stack.push((child, parent.clone()));
                }
            }
            TreeNode::Keyed(_, child) => stack.push((*child, parent)),
            TreeNode::Suspense(suspense) => {
                let children_parent = if suspense.is_revealed() {
                    parent.clone()
                } else {
                    None
                };
                if let SuspenseState::Fallback(fallback) = suspense.state {
                    stack.push((*fallback, parent));
                }
                for child in suspense.children {
                    stack.push((child, children_parent.clone()));
                }
            }
        }
    }
}

pub(super) async fn replace_node<N, E, S>(
    node: &mut TreeNode<N, E>,
    element: Element<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let old_node = mem::replace(node, TreeNode::from(element));
    let visited = visit(node, Work::Create, ctx.clone(), pass).await;
    let mut changes = Changes::default();
    remove(old_node, ctx.attached_parent(), &mut changes);
    chain(visited, changes)
}

/// Replaces the children of an error boundary with its fallback when one of them failed.
/// The changes until the failure are kept, so the children can be removed as they are in the tree.
/// Errors of the fallback are passed on to the next boundary above.
async fn catch<N, E, S>(
    boundary: &mut TreeErrorBoundary<N, E>,
    visited: Visited<N, E>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let failure = match visited {
        Err(failure) if boundary.failed.is_none() => failure,
        visited => return visited,
    };
    let mut changes = failure.changes;
    // Removed components are unmounted, so their effects of this render pass don't run
    for child in boundary.children.drain(..) {
        remove(child, ctx.attached_parent(), &mut changes);
    }
    let dismissed = Arc::new(AtomicBool::new(false));
    boundary.failed = Some(dismissed.clone());
    let dismiss = Dismiss {
        dismissed,
        owner: ctx.owner.clone(),
        signal: pass.signal.clone(),
    };
    let error = CaughtError::new(failure.error, Some(dismiss));
    boundary
        .children
        .push(TreeNode::from((boundary.fallback)(error)));
    changes.merge(create_children(&mut boundary.children, ctx, pass).await)?;
    Ok(changes)
}
