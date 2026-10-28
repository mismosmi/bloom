//! Native GTK4 rendering for bloom.
//!
//! Render a component into a window from your application's `activate` handler:
//! ```no_run
//! use bloom_core::{component, use_state, Element};
//! use bloom_gtk::{prelude::*, render, GtkNode};
//! use bloom_rsx::rsx;
//! use gtk::prelude::*;
//!
//! #[component]
//! fn Counter() -> Result<Element<GtkNode, ()>, ()> {
//!     let count = use_state(|| 0);
//!     Ok(rsx!(
//!         <GtkBox spacing=6 margin=12>
//!             <Label label={count.to_string()} />
//!             <Button on_clicked={move |_| count.update(|count| *count + 1)}>"Increment"</Button>
//!         </GtkBox>
//!     ))
//! }
//!
//! let app = gtk::Application::builder().application_id("org.example.Counter").build();
//! app.connect_activate(|app| {
//!     let window = gtk::ApplicationWindow::new(app);
//!     render(&window, rsx!(<Counter />));
//!     window.present();
//! });
//! app.run();
//! ```
//!
//! Widgets are either typed widgets from [widgets] (`<Button />`, `<Entry />`, ...)
//! or, for any other GTK type, generic elements created through [tag] (`<calendar />`).

mod container;
mod element;
mod node;
mod object_model;
mod value;
mod widget_ref;
pub mod widgets;

use std::{fmt::Debug, sync::Arc};

use bloom_core::{render_loop, Element};
use futures_util::task::{FutureObj, Spawn, SpawnError};
use gtk::{glib, prelude::*};

/// The gtk4 crate bloom-gtk is built on
pub use gtk;

pub use element::{register_tag, tag, GtkElement, GtkElementBuilder, SignalHandler};
pub use node::GtkNode;
pub(crate) use node::NodeKind;
use object_model::Gtk;
pub use value::{Float, PropValue};
pub use widget_ref::WidgetRef;

/// Import `bloom_gtk::prelude::*` wherever you use [bloom-rsx](https://crates.io/crates/bloom-rsx)
/// to render GTK widgets
pub mod prelude {
    /// The `keyed`-function rsx will use for elements with a `key`
    pub use bloom_core::keyed;
    /// GTK's traits, for using widgets in signal handlers and effects
    pub use gtk::prelude::*;

    /// The `tag`-function rsx will use for lowercase tags
    pub use super::tag;
    pub use super::widgets::{
        Button, CheckButton, Entry, Expander, FlowBox, Frame, GtkBox, Image, Label, LevelBar,
        ListBox, ListBoxRow, ProgressBar, Revealer, ScrolledWindow, Separator, Spinner, Switch,
        ToggleButton,
    };
}

/// Runs suspended component renders on the GTK main context
struct GlibSpawner;

impl Spawn for GlibSpawner {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        glib::MainContext::default().spawn(future);
        Ok(())
    }
}

/// Render element into root, replacing root's child.
/// Root is usually a `gtk::Window`, but can be any widget that can hold children,
/// such as a `gtk::Box` that is part of a UI built without bloom.
///
/// Must be called on the GTK main thread after GTK has been initialized.
/// The UI is rendered and updated by a task on the default main context, so it
/// appears once the main loop runs. The returned handle resolves with the error
/// if a component's render function fails.
pub fn render<E>(
    root: &impl IsA<gtk::Widget>,
    element: Element<GtkNode, E>,
) -> glib::JoinHandle<Result<(), E>>
where
    E: Debug + Send + 'static,
{
    let root_node = Arc::new(GtkNode::root());
    let object_model = Gtk::new(&root_node, root.clone().upcast());

    glib::MainContext::default().spawn_local(async move {
        let result = render_loop(root_node, element, GlibSpawner, object_model).await;
        if let Err(error) = &result {
            glib::g_critical!("bloom-gtk", "Render loop error: {:?}", error);
        }
        result
    })
}
