mod async_memo;
mod callback;
mod commit;
mod component;
mod context;
mod effect;
mod element;
mod fragment;
mod hook;
mod identify;
mod key;
mod memo;
mod object_ref;
mod render_context;
mod render_loop;
mod render_stream;
mod result;
mod spawn;
mod state;
mod suspense;

pub use async_memo::use_async_memo;
pub use bloom_macro::Identify;
/// Turns a function into a component.
/// The function arguments become the props of the component.
/// Props of type `Option<T>` are optional, all other props are required.
/// The generated builder (from [bon](https://docs.rs/bon)) accepts anything that converts into the prop type.
/// ```
/// use bloom_core::{component, Element};
///
/// #[component]
/// fn Greeting(name: String, times: Option<usize>) -> Result<Element<String, ()>, ()> {
///     Ok(name.repeat(times.unwrap_or(1)).into())
/// }
///
/// #[component]
/// fn Labelled<T: ToString + PartialEq + Send + Sync + 'static>(
///     label: T,
/// ) -> Result<Element<String, ()>, ()> {
///     Ok(label.to_string().into())
/// }
///
/// let greeting: Element<String, ()> = Greeting::builder().name("Hi").times(2usize).build().into();
/// let label: Element<String, ()> = Labelled::<i32>::builder().label(42).build().into();
/// ```
///
/// Leaving out a required prop is a compile-time error:
/// ```compile_fail,E0599
/// use bloom_core::{component, Element};
/// #[component]
/// fn Greeting(name: String) -> Result<Element<String, ()>, ()> {
///     Ok(name.into())
/// }
/// let greeting = Greeting::builder().build();
/// ```
pub use bloom_macro::component;
pub use callback::use_callback;
pub use component::Component;
pub use context::{_get_context, Provider, ProviderBuilder, use_context};
pub use effect::use_effect;
pub use element::{Element, keyed};
pub use fragment::{Fragment, FragmentBuilder};
pub use futures_util;
pub use identify::{ByHash, Identify};
pub use key::Key;
pub use memo::use_memo;
pub use object_ref::{use_ref, use_ref_with_default};
pub use render_loop::{ObjectModel, render_loop};
pub use render_stream::{NodeStream, render_stream};
pub use result::Result;
pub use spawn::{Spawner, use_spawner};
pub use state::{State, use_state};
pub use suspense::Suspense;

#[doc(hidden)]
pub mod __private {
    pub use async_trait::async_trait;
    pub use bon;
}
