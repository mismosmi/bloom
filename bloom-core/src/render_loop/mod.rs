//! The interactive render loop.
//! A render pass visits the tree (`visit`, `children`), rendering components on the spawner
//! (`render`), and commits the collected changes in one go.
use crate::MaybeSpawn;
use crate::{MaybeSend, MaybeSync};

mod children;
mod render;
#[cfg(test)]
mod tests;
mod tree;
mod visit;

use std::sync::Arc;

use async_channel::{Sender, bounded};
use futures_util::{Future, future};

use crate::{
    Element, Spawner,
    commit::{Changes, Cleanups},
    component::AnyComponent,
    context::ContextMap,
    hook::Hook,
    render_context::RenderContext,
};

use tree::{TreeNode, TreeRoot};
use visit::visit;

type RenderOutput<N, E> = (Result<Element<N, E>, E>, Hook);
type RenderFuture<N, E> = crate::platform::BoxFuture<'static, RenderOutput<N, E>>;
type Visit<'a, N, E> = crate::platform::BoxFuture<'a, Result<Changes<N>, E>>;

/// Levels of nested nodes that are visited in one task before continuing in a new one
const TASK_DEPTH: usize = 32;

/// What a render pass does with a subtree
enum Work<N, E>
where
    N: From<String>,
{
    Create,
    Reload,
    Update(Element<N, E>),
}

/// What a render pass does with a component
enum ComponentWork<N, E> {
    Create,
    Reload,
    NewProps(Arc<dyn AnyComponent<Node = N, Error = E>>),
}

/// Shared by all tasks of the render loop, each task gets its own clone
#[derive(Clone)]
struct Pass {
    spawner: Spawner,
    signal: Sender<()>,
}

pub trait ObjectModel {
    type Node;
    fn start(&mut self) -> impl Future<Output = ()> + MaybeSend {
        // Do nothing by default
        future::ready(())
    }
    fn create(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    );
    fn remove(&mut self, node: &Arc<Self::Node>, parent: &Arc<Self::Node>);
    /// Moves an existing node within its parent so it is placed before the sibling,
    /// or at the end of the parent if there is no sibling.
    /// This is used to reorder keyed lists and must keep the node and its children intact.
    fn move_node(
        &mut self,
        node: &Arc<Self::Node>,
        parent: &Arc<Self::Node>,
        sibling: &Option<Arc<Self::Node>>,
    );
    fn update(&mut self, node: &Arc<Self::Node>, next: &Arc<Self::Node>);
    fn finalize(&mut self) -> impl Future<Output = ()> + MaybeSend {
        // Do nothing by default
        future::ready(())
    }
    fn subscribe(&mut self, _signal: Sender<()>) {
        // do nothing by default
    }
    fn get_context(&mut self) -> ContextMap {
        Arc::default()
    }
    fn set_context(&mut self, _ctx: ContextMap) {
        // do nothing by default
    }
}

/// render_loop can be used to implement interactive renderers on top of bloom-core.
/// The primary renderer is the bloom-client library which implements client side rendering
/// in the browser on top of bloom using webassembly.
/// The root is the root node in the host environment which will contain the rendered UI.
/// In web-environments the root might be a div within the body that is part of the server-sent html:
/// ```html
/// <html>
///   <body>
///     <div id="root"></div>
///   </body>
/// </html>
/// ```
///
///
/// The element is the root of the component tree that represents the intended UI.
/// This is usually a component.
///
/// The spawner-argument is
/// an object that implements [`MaybeSpawn`] so bloom can spawn async tasks on
/// arbitrary task runners such as tokio or async-std.
/// Every component that renders is rendered and diffed in its own task on the spawner, so with a
/// multi-threaded spawner sibling components render in parallel.
/// Each of these tasks gets its own clone of the spawner, which is shared with the tasks
/// it spawns, so the spawner has to be `Clone`, [`MaybeSend`] and [`MaybeSync`].
/// Browser WebAssembly uses `LocalSpawn`; other targets use `Spawn`.
///
/// Each render pass has two phases:
/// 1. Rendering visits the tree and collects the changes to the object model in document order,
///    the host environment is not touched.
///    A component that does not finish its render on the first poll suspends and its
///    result is picked up by a later render pass.
/// 2. The commit runs synchronously in one go after `ObjectModel::start`:
///    first the cleanups of removed components and of effects that run again,
///    then the queued changes to the object model, and finally the effects.
///
/// The future returned by render_loop is not `Send` because it keeps the cleanups of effects,
/// which can hold values that are not `Send` such as JavaScript objects.
/// Run it on a single-threaded executor, e.g. with `spawn_local`.
///
/// The object model is the interface to the host environment.
/// It is notified whenever a node needs to be created, removed or updated:
/// ```no_run
/// # use std::sync::Arc;
/// # use async_trait::async_trait;
/// # use bloom_core::{Component, Element};
/// use bloom_core::{render_loop, ObjectModel};
///
/// struct PrintObjectModel;
///
/// impl ObjectModel for PrintObjectModel {
///     type Node = String;
///
///     fn create(&mut self, node: &Arc<String>, _parent: &Arc<String>, _sibling: &Option<Arc<String>>) {
///         println!("create {:?}", node);
///     }
///
///     fn move_node(&mut self, node: &Arc<String>, _parent: &Arc<String>, _sibling: &Option<Arc<String>>) {
///         println!("Moved {:?}", node);
///     }
///
///     fn remove(&mut self, node: &Arc<String>, _parent: &Arc<String>) {
///         println!("remove {:?}", node);
///     }
///
///     fn update(&mut self, node: &Arc<String>, next: &Arc<String>) {
///         println!("update {:?} to {:?}", node, next);
///     }
/// }
///
/// # #[derive(PartialEq)]
/// # struct MyComponent;
/// # #[async_trait]
/// # impl Component for MyComponent {
/// #     type Node = String;
/// #     type Error = ();
/// #     async fn render(self: Arc<Self>) -> Result<Element<String, ()>, ()> {
/// #         Ok("Hello".into())
/// #     }
/// # }
/// let root = Arc::new(String::from("root"));
/// let spawner = futures_executor::ThreadPool::new().unwrap();
///
/// futures_executor::block_on(render_loop(root, MyComponent.into(), spawner, PrintObjectModel))
///     .unwrap();
/// ```
pub async fn render_loop<N, E, S, P>(
    root: Arc<N>,
    element: Element<N, E>,
    spawner: S,
    mut object_model: P,
) -> Result<(), E>
where
    N: From<String> + MaybeSend + MaybeSync + 'static,
    E: MaybeSend + 'static,
    S: MaybeSpawn + Clone + MaybeSend + MaybeSync + 'static,
    P: ObjectModel<Node = N>,
{
    let mut tree_root = TreeRoot(TreeNode::from(element));

    let (signal_sender, signal_receiver) = bounded::<()>(1);

    object_model.subscribe(signal_sender.clone());

    signal_sender
        .try_send(())
        .expect("Failed to send message to trigger initial render");

    let pass = Pass {
        spawner: Spawner::new(spawner),
        signal: signal_sender,
    };
    let mut cleanups = Cleanups::default();
    let mut created = false;

    while signal_receiver.recv().await.is_ok() {
        let ctx = RenderContext::new(root.clone(), None, object_model.get_context());
        // The first pass creates the whole tree, later passes only reload what changed
        let work = if created { Work::Reload } else { Work::Create };
        created = true;
        let changes = visit(&mut tree_root.0, work, ctx, &pass).await?;

        object_model.start().await;
        cleanups.commit(changes, &mut object_model);
        object_model.finalize().await;
    }

    Ok(())
}
