use std::{any::Any, fmt::Debug};

use gtk::{glib, prelude::*};

use crate::{element::GtkElement, WidgetRef};

/// The node type for rendering bloom components to GTK4 widgets.
/// A node only describes a widget, it is plain data that can be sent between threads
/// (as bloom-core requires). The actual `gtk::Widget`s are created and owned by the renderer
/// on the GTK main thread.
///
/// Nodes are created through the typed widgets in [crate::widgets]:
/// ```
/// use bloom_core::Element;
/// use bloom_gtk::{prelude::*, GtkNode};
/// use bloom_rsx::rsx;
///
/// let element: Element<GtkNode, ()> = rsx!(
///     <GtkBox spacing=8>
///         <Label label="Hello" />
///         <Button on_clicked={|_| println!("clicked")}>"Click me"</Button>
///     </GtkBox>
/// );
/// ```
/// through the generic [crate::tag] function for any other widget type,
/// or from strings, which are rendered as `gtk::Label`s.
pub struct GtkNode(pub(crate) NodeKind);

pub(crate) enum NodeKind {
    Typed(Box<dyn WidgetSpec>),
    Element(GtkElement),
    Text(String),
    /// The widget bloom renders into
    Root,
}

impl GtkNode {
    pub(crate) fn typed(spec: impl WidgetSpec) -> Self {
        Self(NodeKind::Typed(Box::new(spec)))
    }

    pub(crate) fn root() -> Self {
        Self(NodeKind::Root)
    }

    /// Whether a widget created for self can be updated to next in place
    pub(crate) fn same_kind(&self, next: &Self) -> bool {
        match (&self.0, &next.0) {
            (NodeKind::Typed(a), NodeKind::Typed(b)) => {
                a.as_any().type_id() == b.as_any().type_id()
            }
            (NodeKind::Element(a), NodeKind::Element(b)) => a.kind() == b.kind(),
            (NodeKind::Text(_), NodeKind::Text(_)) => true,
            _ => false,
        }
    }

    pub(crate) fn widget_ref(&self) -> Option<&WidgetRef> {
        match &self.0 {
            NodeKind::Typed(spec) => spec.widget_ref(),
            NodeKind::Element(element) => element.widget_ref(),
            _ => None,
        }
    }

    /// Names of the signals this node has handlers for
    pub(crate) fn signals(&self) -> Vec<&str> {
        match &self.0 {
            NodeKind::Typed(spec) => spec.signals(),
            NodeKind::Element(element) => element.signals(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn emit(&self, signal: &str, widget: &gtk::Widget) {
        match &self.0 {
            NodeKind::Typed(spec) => spec.emit(signal, widget),
            NodeKind::Element(element) => element.emit(signal, widget),
            _ => {}
        }
    }
}

impl From<String> for GtkNode {
    fn from(value: String) -> Self {
        Self(NodeKind::Text(value))
    }
}

impl PartialEq for GtkNode {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (NodeKind::Typed(a), NodeKind::Typed(b)) => a.dyn_eq(b.as_ref()),
            (NodeKind::Element(a), NodeKind::Element(b)) => a == b,
            (NodeKind::Text(a), NodeKind::Text(b)) => a == b,
            _ => false,
        }
    }
}

impl Debug for GtkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            NodeKind::Typed(spec) => f.write_str(spec.type_name()),
            NodeKind::Element(element) => element.fmt(f),
            NodeKind::Text(text) => f.debug_tuple("Text").field(text).finish(),
            NodeKind::Root => f.write_str("Root"),
        }
    }
}

/// Implemented by the typed widgets in [crate::widgets].
/// A spec describes the properties and signal handlers of one widget type
/// and knows how to create and patch that widget.
pub(crate) trait WidgetSpec: Send + Sync + Any {
    fn create(&self) -> gtk::Widget;
    /// Set the properties that changed since prev, which is always of the same type as self
    fn apply(&self, widget: &gtk::Widget, prev: Option<&dyn WidgetSpec>);
    fn signals(&self) -> Vec<&str>;
    fn emit(&self, signal: &str, widget: &gtk::Widget);
    fn widget_ref(&self) -> Option<&WidgetRef>;
    fn type_name(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn dyn_eq(&self, other: &dyn WidgetSpec) -> bool;
}

/// The value a signal handler returns to GTK.
/// Handlers can't return values themselves, so signals that expect one get the
/// "not handled" default (`false` for the common boolean signals like `close-request`).
pub(crate) fn default_return(widget: &gtk::Widget, signal: &str) -> Option<glib::Value> {
    let name = signal.split("::").next().unwrap_or(signal);
    let id = glib::subclass::signal::SignalId::lookup(name, widget.type_())?;
    let return_type = id.query().return_type().type_();
    if return_type == glib::Type::UNIT {
        None
    } else if return_type == glib::Type::BOOL {
        Some(false.to_value())
    } else {
        Some(glib::Value::from_type(return_type))
    }
}
