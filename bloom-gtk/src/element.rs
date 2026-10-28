use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    sync::Arc,
};

use bloom_core::Element;
use gtk::{glib, prelude::*};

use crate::{GtkNode, NodeKind, WidgetRef};

/// Handler for signals of generic elements. It receives the widget that emitted the signal.
pub type SignalHandler = Box<dyn Fn(&gtk::Widget) + Send + Sync + 'static>;

/// A widget of any GTK type, created through [tag].
/// Properties are passed as strings and converted to the property's type at runtime,
/// so prefer the typed widgets in [crate::widgets] where available.
pub struct GtkElement {
    kind: &'static str,
    attributes: BTreeMap<String, String>,
    handlers: HashMap<String, SignalHandler>,
    widget_ref: Option<Arc<WidgetRef>>,
}

impl GtkElement {
    pub(crate) fn kind(&self) -> &'static str {
        self.kind
    }

    pub(crate) fn widget_ref(&self) -> Option<&WidgetRef> {
        self.widget_ref.as_deref()
    }

    pub(crate) fn signals(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    pub(crate) fn emit(&self, signal: &str, widget: &gtk::Widget) {
        if let Some(handler) = self.handlers.get(signal) {
            handler(widget);
        }
    }

    pub(crate) fn create(&self) -> gtk::Widget {
        let ty = widget_type(self.kind);
        glib::Object::with_type(ty)
            .downcast()
            .unwrap_or_else(|_| panic!("{} is not a widget", ty.name()))
    }

    pub(crate) fn apply(&self, widget: &gtk::Widget, prev: Option<&Self>) {
        for (name, raw) in &self.attributes {
            if prev.and_then(|prev| prev.attributes.get(name)) != Some(raw) {
                widget.set_property_from_value(name, &crate::value::parse(widget, name, raw));
            }
        }
        if let Some(prev) = prev {
            for name in prev.attributes.keys() {
                if !self.attributes.contains_key(name) {
                    crate::value::reset(widget, name);
                }
            }
        }
    }

    pub fn children<E>(self, children: Vec<Element<GtkNode, E>>) -> Element<GtkNode, E> {
        Element::Node(self.into(), children)
    }
}

impl PartialEq for GtkElement {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.attributes == other.attributes
            && self.handlers.is_empty()
            && other.handlers.is_empty()
            && self.widget_ref == other.widget_ref
    }
}

impl Debug for GtkElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GtkElement")
            .field("kind", &self.kind)
            .field("attributes", &self.attributes)
            .field("handlers", &self.handlers.keys())
            .finish()
    }
}

impl From<GtkElement> for GtkNode {
    fn from(element: GtkElement) -> Self {
        GtkNode(NodeKind::Element(element))
    }
}

impl<E> From<GtkElement> for Element<GtkNode, E> {
    fn from(element: GtkElement) -> Self {
        Element::Node(element.into(), Vec::new())
    }
}

/// Create a widget of any GTK type. This is what rsx uses for lowercase tags,
/// so `<spin_button digits=2 />` creates a `GtkSpinButton`:
/// the tag is converted to the type name by capitalizing its parts and prefixing `Gtk`.
/// Types from other libraries can be made available with [register_tag].
/// ```
/// use bloom_core::Element;
/// use bloom_gtk::{prelude::*, GtkNode};
/// use bloom_rsx::rsx;
///
/// let element: Element<GtkNode, ()> =
///     rsx!(<level_bar value=0.5 on_offset_changed={|_| println!("changed")} />);
/// ```
pub fn tag(kind: &'static str) -> GtkElementBuilder {
    GtkElementBuilder(GtkElement {
        kind,
        attributes: BTreeMap::new(),
        handlers: HashMap::new(),
        widget_ref: None,
    })
}

pub struct GtkElementBuilder(GtkElement);

impl GtkElementBuilder {
    /// Set a property. The value is converted to the property's type when the widget is rendered.
    pub fn attr(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        self.0.attributes.insert(name.into(), value.to_string());
        self
    }

    /// Connect a handler to a signal.
    /// Signal names may use underscores instead of dashes, e.g. `value_changed`.
    pub fn on<F>(mut self, signal: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&gtk::Widget) + Send + Sync + 'static,
    {
        self.0.handlers.insert(signal.into(), Box::new(handler));
        self
    }

    /// Get a reference to the widget. rsx calls this for the `ref` attribute.
    pub fn dom_ref(mut self, widget_ref: Arc<WidgetRef>) -> Self {
        self.0.widget_ref = Some(widget_ref);
        self
    }

    pub fn build(self) -> GtkElement {
        self.0
    }
}

thread_local! {
    static TAGS: RefCell<HashMap<&'static str, glib::Type>> = RefCell::new(HashMap::new());
}

/// Make a widget type from another library available as a tag:
/// ```ignore
/// bloom_gtk::register_tag("adw_action_row", adw::ActionRow::static_type());
/// ```
/// Must be called on the GTK main thread.
pub fn register_tag(tag: &'static str, ty: glib::Type) {
    TAGS.with(|tags| tags.borrow_mut().insert(tag, ty));
}

fn widget_type(kind: &'static str) -> glib::Type {
    if let Some(ty) = TAGS.with(|tags| tags.borrow().get(kind).copied()) {
        return ty;
    }
    let mut name = String::from("Gtk");
    for part in kind.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            name.extend(first.to_uppercase());
            name.push_str(chars.as_str());
        }
    }
    glib::Type::from_name(&name)
        .or_else(|| {
            // GTK registers its types lazily
            gtk::test_register_all_types();
            glib::Type::from_name(&name)
        })
        .unwrap_or_else(|| panic!("Unknown widget type {name} for tag {kind}"))
}
