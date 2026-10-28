use gtk::{glib, prelude::*};

/// A value that can be set as a GObject property of a typed widget.
pub trait PropValue: ToValue + PartialEq + Send + Sync + 'static {
    /// Whether the property value currently on the widget equals self
    fn matches(&self, value: &glib::Value) -> bool;
}

macro_rules! prop_value {
    ($($ty:ty),* $(,)?) => {
        $(impl PropValue for $ty {
            fn matches(&self, value: &glib::Value) -> bool {
                value.get::<$ty>().is_ok_and(|current| current == *self)
            }
        })*
    };
}

prop_value!(
    bool,
    i32,
    u32,
    f32,
    f64,
    gtk::Align,
    gtk::Justification,
    gtk::Orientation,
    gtk::PolicyType,
    gtk::RevealerTransitionType,
    gtk::SelectionMode,
    gtk::InputPurpose,
);

/// The value of `float` properties such as `xalign`.
/// It converts from both `f32` and `f64`, so plain literals like `xalign=0.5` work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float(pub f32);

impl From<f32> for Float {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<f64> for Float {
    fn from(value: f64) -> Self {
        Self(value as f32)
    }
}

impl ToValue for Float {
    fn to_value(&self) -> glib::Value {
        self.0.to_value()
    }

    fn value_type(&self) -> glib::Type {
        f32::static_type()
    }
}

impl PropValue for Float {
    fn matches(&self, value: &glib::Value) -> bool {
        value.get::<f32>().is_ok_and(|current| current == self.0)
    }
}

impl PropValue for String {
    fn matches(&self, value: &glib::Value) -> bool {
        value
            .get::<Option<String>>()
            .is_ok_and(|current| current.as_ref() == Some(self))
    }
}

impl PropValue for Vec<String> {
    fn matches(&self, value: &glib::Value) -> bool {
        value
            .get::<Vec<String>>()
            .is_ok_and(|current| current == *self)
    }
}

/// Set a property to next unless the widget already has that value,
/// or reset it to its default if it was set before (`prev`) and is unset now.
/// Comparing with the widget's current value keeps edits in progress intact,
/// e.g. setting the text an `Entry` already contains does not move the cursor.
pub(crate) fn patch<V: PropValue>(
    widget: &gtk::Widget,
    name: &str,
    next: &Option<V>,
    prev: Option<&Option<V>>,
) {
    match next {
        Some(value) => {
            if !value.matches(&widget.property_value(name)) {
                widget.set_property(name, value);
            }
        }
        None => {
            if prev.is_some_and(Option::is_some) {
                reset(widget, name);
            }
        }
    }
}

pub(crate) fn reset(widget: &gtk::Widget, name: &str) {
    let default = widget
        .find_property(name)
        .unwrap_or_else(|| panic!("{} has no property {name}", widget.type_().name()))
        .default_value()
        .clone();
    widget.set_property_from_value(name, &default);
}

/// Parse the string value of a generic attribute into the type of the property.
pub(crate) fn parse(widget: &gtk::Widget, name: &str, raw: &str) -> glib::Value {
    let pspec = widget
        .find_property(name)
        .unwrap_or_else(|| panic!("{} has no property {name}", widget.type_().name()));
    let ty = pspec.value_type();
    let invalid = || -> ! {
        panic!(
            "Cannot set property {name} of {} to {raw:?}: expected {}",
            widget.type_().name(),
            ty.name()
        )
    };
    if ty == String::static_type() {
        raw.to_value()
    } else if ty == bool::static_type() {
        raw.parse::<bool>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == i32::static_type() {
        raw.parse::<i32>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == u32::static_type() {
        raw.parse::<u32>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == i64::static_type() {
        raw.parse::<i64>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == u64::static_type() {
        raw.parse::<u64>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == f32::static_type() {
        raw.parse::<f32>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty == f64::static_type() {
        raw.parse::<f64>().unwrap_or_else(|_| invalid()).to_value()
    } else if ty.is_a(glib::Type::ENUM) {
        glib::EnumClass::with_type(ty)
            .and_then(|class| class.to_value_by_nick(raw))
            .unwrap_or_else(|| invalid())
    } else if ty == <Vec<String>>::static_type() {
        raw.split_whitespace().collect::<Vec<_>>().to_value()
    } else {
        invalid()
    }
}
