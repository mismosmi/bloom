use std::{
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    sync::atomic::{AtomicU64, Ordering},
};

use gtk::prelude::*;

thread_local! {
    static WIDGETS: RefCell<HashMap<u64, gtk::Widget>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A reference to the GTK widget a node is rendered to.
/// Widgets can only be used on the GTK main thread, so the reference itself only holds an id
/// and can be stored in component state.
/// Obtain one with `use_ref` and pass it to a widget's `ref`:
/// ```no_run
/// # use std::sync::Arc;
/// # use async_trait::async_trait;
/// # use bloom_core::{Component, Element};
/// use bloom_core::{use_effect, use_ref};
/// use bloom_gtk::{prelude::*, GtkNode, WidgetRef};
/// use bloom_rsx::rsx;
///
/// # #[derive(PartialEq)]
/// # struct MyComponent;
/// # #[async_trait]
/// # impl Component for MyComponent {
/// #     type Node = GtkNode;
/// #     type Error = ();
/// #     async fn render(self: Arc<Self>) -> Result<Element<GtkNode, ()>, ()> {
/// let entry = use_ref::<WidgetRef>();
///
/// use_effect(entry.clone(), |entry| {
///     if let Some(entry) = entry.get_as::<gtk::Entry>() {
///         entry.grab_focus();
///     }
/// });
///
/// Ok(rsx!(<Entry ref={entry} />))
/// #     }
/// # }
/// ```
#[derive(Debug)]
pub struct WidgetRef(u64);

impl Default for WidgetRef {
    fn default() -> Self {
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl WidgetRef {
    pub(crate) fn set(&self, widget: &gtk::Widget) {
        WIDGETS.with(|widgets| widgets.borrow_mut().insert(self.0, widget.clone()));
    }

    /// Get the widget. Returns None before the node has been rendered
    /// and when called on any thread but the GTK main thread.
    pub fn get(&self) -> Option<gtk::Widget> {
        WIDGETS.with(|widgets| widgets.borrow().get(&self.0).cloned())
    }

    /// Get the widget as a specific widget type
    pub fn get_as<W: IsA<gtk::Widget>>(&self) -> Option<W> {
        self.get().and_then(|widget| widget.downcast().ok())
    }
}

impl Drop for WidgetRef {
    fn drop(&mut self) {
        let _ = WIDGETS.try_with(|widgets| widgets.borrow_mut().remove(&self.0));
    }
}

impl PartialEq for WidgetRef {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for WidgetRef {}

impl Hash for WidgetRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
