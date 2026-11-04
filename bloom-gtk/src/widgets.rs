//! Typed GTK widgets. In rsx they are used like components:
//! ```
//! use bloom_core::Element;
//! use bloom_gtk::{prelude::*, GtkNode};
//! use bloom_rsx::rsx;
//!
//! let element: Element<GtkNode, ()> = rsx!(
//!     <GtkBox orientation={gtk::Orientation::Vertical} spacing=6 margin=12>
//!         <Label label="Name" xalign=0.0 />
//!         <Entry placeholder_text="Jane Doe" on_changed={|entry| println!("{}", entry.text())} />
//!         <Button css_classes={["suggested-action"]} on_clicked={|_| println!("saved")}>
//!             "Save"
//!         </Button>
//!     </GtkBox>
//! );
//! ```
//! but they render directly to a widget instead of to other elements.
//! Every property is optional and only set on the widget if it is passed,
//! otherwise the widget keeps GTK's default. Signal handlers receive the widget
//! that emitted the signal.
//! Only containers accept children, so `<Label>"text"</Label>` does not compile.

use std::{any::Any, sync::Arc};

use bloom_core::Element;
use gtk::{glib, prelude::*};

use crate::{
    node::WidgetSpec,
    value::{patch, Float},
    GtkNode, WidgetRef,
};

/// The builder rsx uses for typed widgets: `<Button label="Hi" />` becomes
/// `Button::builder().label("Hi").build()`.
pub struct WidgetBuilder<T, E> {
    spec: T,
    children: Vec<Element<GtkNode, E>>,
}

type Handler<W> = Box<dyn Fn(&W) + Send + Sync + 'static>;

/// Properties every widget has
#[derive(Default, PartialEq)]
struct CommonProps {
    name: Option<String>,
    hexpand: Option<bool>,
    vexpand: Option<bool>,
    halign: Option<gtk::Align>,
    valign: Option<gtk::Align>,
    margin_start: Option<i32>,
    margin_end: Option<i32>,
    margin_top: Option<i32>,
    margin_bottom: Option<i32>,
    width_request: Option<i32>,
    height_request: Option<i32>,
    sensitive: Option<bool>,
    visible: Option<bool>,
    focusable: Option<bool>,
    opacity: Option<f64>,
    tooltip_text: Option<String>,
    css_classes: Option<Vec<String>>,
    widget_ref: Option<Arc<WidgetRef>>,
}

impl CommonProps {
    fn apply(&self, widget: &gtk::Widget, prev: Option<&Self>) {
        macro_rules! patch_all {
            ($($prop:ident: $name:literal),*) => {
                $(patch(widget, $name, &self.$prop, prev.map(|prev| &prev.$prop));)*
            };
        }
        patch_all!(
            name: "name",
            hexpand: "hexpand",
            vexpand: "vexpand",
            halign: "halign",
            valign: "valign",
            margin_start: "margin-start",
            margin_end: "margin-end",
            margin_top: "margin-top",
            margin_bottom: "margin-bottom",
            width_request: "width-request",
            height_request: "height-request",
            sensitive: "sensitive",
            visible: "visible",
            focusable: "focusable",
            opacity: "opacity",
            tooltip_text: "tooltip-text",
            css_classes: "css-classes"
        );
    }
}

macro_rules! common_setters {
    ($($prop:ident: $ty:ty),*) => {
        $(
            #[doc = concat!("Set the `", stringify!($prop), "` property")]
            pub fn $prop(mut self, value: impl Into<$ty>) -> Self {
                self.spec.common.$prop = Some(value.into());
                self
            }
        )*

        /// Set the margin on all four sides
        pub fn margin(mut self, value: i32) -> Self {
            let common = &mut self.spec.common;
            common.margin_start = Some(value);
            common.margin_end = Some(value);
            common.margin_top = Some(value);
            common.margin_bottom = Some(value);
            self
        }

        /// Set the style classes, e.g. `css_classes={["card", "suggested-action"]}`
        pub fn css_classes<I>(mut self, classes: I) -> Self
        where
            I: IntoIterator,
            I::Item: Into<String>,
        {
            self.spec.common.css_classes = Some(classes.into_iter().map(Into::into).collect());
            self
        }

        /// Get a reference to the widget. rsx calls this for the `ref` prop.
        pub fn dom_ref(mut self, widget_ref: Arc<WidgetRef>) -> Self {
            self.spec.common.widget_ref = Some(widget_ref);
            self
        }

        pub fn build(self) -> Element<GtkNode, E> {
            Element::Node(GtkNode::typed(self.spec), self.children)
        }
    };
}

macro_rules! widgets {
    ($(
        $(#[$meta:meta])*
        $name:ident($gtk:ty, $kind:ident) {
            props { $($prop:ident: $ty:ty),* $(,)? }
            signals { $($handler:ident: $signal:literal),* $(,)? }
        }
    )*) => {$(
        $(#[$meta])*
        #[doc = ""]
        #[doc = concat!("Renders a [", stringify!($gtk), "].")]
        #[derive(Default)]
        pub struct $name {
            common: CommonProps,
            $($prop: Option<$ty>,)*
            $($handler: Option<Handler<$gtk>>,)*
        }

        impl $name {
            pub fn builder<E>() -> WidgetBuilder<Self, E> {
                WidgetBuilder {
                    spec: Self::default(),
                    children: Vec::new(),
                }
            }
        }

        impl<E> WidgetBuilder<$name, E> {
            common_setters!(
                name: String,
                hexpand: bool,
                vexpand: bool,
                halign: gtk::Align,
                valign: gtk::Align,
                margin_start: i32,
                margin_end: i32,
                margin_top: i32,
                margin_bottom: i32,
                width_request: i32,
                height_request: i32,
                sensitive: bool,
                visible: bool,
                focusable: bool,
                opacity: f64,
                tooltip_text: String
            );

            $(
                #[doc = concat!("Set the `", stringify!($prop), "` property")]
                pub fn $prop(mut self, value: impl Into<$ty>) -> Self {
                    self.spec.$prop = Some(value.into());
                    self
                }
            )*

            $(
                #[doc = concat!("Handle the `", $signal, "` signal")]
                pub fn $handler(mut self, handler: impl Fn(&$gtk) + Send + Sync + 'static) -> Self {
                    self.spec.$handler = Some(Box::new(handler));
                    self
                }
            )*
        }

        widgets!(@$kind $name);

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.common == other.common
                    $(&& self.$prop == other.$prop)*
                    $(&& self.$handler.is_none() && other.$handler.is_none())*
            }
        }

        impl WidgetSpec for $name {
            fn create(&self) -> gtk::Widget {
                glib::Object::new::<$gtk>().upcast()
            }

            fn apply(&self, widget: &gtk::Widget, prev: Option<&dyn WidgetSpec>) {
                let prev = prev.and_then(|prev| prev.as_any().downcast_ref::<Self>());
                self.common.apply(widget, prev.map(|prev| &prev.common));
                $(patch(
                    widget,
                    &stringify!($prop).replace('_', "-"),
                    &self.$prop,
                    prev.map(|prev| &prev.$prop),
                );)*
                let _ = prev;
            }

            fn signals(&self) -> Vec<&str> {
                #[allow(unused_mut)]
                let mut signals = Vec::new();
                $(if self.$handler.is_some() {
                    signals.push($signal);
                })*
                signals
            }

            fn emit(&self, signal: &str, widget: &gtk::Widget) {
                $(if signal == $signal {
                    if let (Some(handler), Some(widget)) =
                        (&self.$handler, widget.downcast_ref::<$gtk>())
                    {
                        handler(widget);
                    }
                })*
                let _ = (signal, widget);
            }

            fn widget_ref(&self) -> Option<&WidgetRef> {
                self.common.widget_ref.as_deref()
            }

            fn type_name(&self) -> &'static str {
                stringify!($name)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn dyn_eq(&self, other: &dyn WidgetSpec) -> bool {
                other.as_any().downcast_ref::<Self>() == Some(self)
            }
        }
    )*};
    (@container $name:ident) => {
        impl<E> WidgetBuilder<$name, E> {
            /// rsx passes the children of the tag here
            pub fn children(mut self, children: Vec<Element<GtkNode, E>>) -> Self {
                self.children = children;
                self
            }
        }
    };
    (@leaf $name:ident) => {};
}

widgets! {
    /// Displays text. Strings in rsx are rendered as plain labels, too.
    Label(gtk::Label, leaf) {
        props {
            label: String,
            use_markup: bool,
            selectable: bool,
            wrap: bool,
            justify: gtk::Justification,
            xalign: Float,
            yalign: Float,
            width_chars: i32,
            max_width_chars: i32,
        }
        signals {}
    }

    /// Arranges its children in a row or column.
    /// Named `GtkBox` so it does not shadow `std::boxed::Box`.
    GtkBox(gtk::Box, container) {
        props {
            orientation: gtk::Orientation,
            spacing: i32,
            homogeneous: bool,
        }
        signals {}
    }

    /// A button. Its child is usually a string: `<Button>"Save"</Button>`.
    Button(gtk::Button, container) {
        props {
            label: String,
            icon_name: String,
            has_frame: bool,
            use_underline: bool,
        }
        signals {
            on_clicked: "clicked",
        }
    }

    /// A button that stays pressed until it is clicked again.
    ToggleButton(gtk::ToggleButton, container) {
        props {
            label: String,
            icon_name: String,
            active: bool,
            has_frame: bool,
            use_underline: bool,
        }
        signals {
            on_toggled: "toggled",
            on_clicked: "clicked",
        }
    }

    /// A check box with an optional label.
    CheckButton(gtk::CheckButton, container) {
        props {
            label: String,
            active: bool,
            inconsistent: bool,
            use_underline: bool,
        }
        signals {
            on_toggled: "toggled",
        }
    }

    /// An on/off switch.
    Switch(gtk::Switch, leaf) {
        props {
            active: bool,
        }
        signals {
            on_notify_active: "notify::active",
        }
    }

    /// A single line text input.
    Entry(gtk::Entry, leaf) {
        props {
            text: String,
            placeholder_text: String,
            editable: bool,
            visibility: bool,
            max_length: i32,
            input_purpose: gtk::InputPurpose,
        }
        signals {
            on_changed: "changed",
            on_activate: "activate",
        }
    }

    /// Shows a level, e.g. a battery charge.
    LevelBar(gtk::LevelBar, leaf) {
        props {
            value: f64,
            min_value: f64,
            max_value: f64,
            inverted: bool,
        }
        signals {}
    }

    /// Shows the progress of a long running operation.
    ProgressBar(gtk::ProgressBar, leaf) {
        props {
            fraction: f64,
            text: String,
            show_text: bool,
            pulse_step: f64,
            inverted: bool,
        }
        signals {}
    }

    /// A spinning activity indicator.
    Spinner(gtk::Spinner, leaf) {
        props {
            spinning: bool,
        }
        signals {}
    }

    /// Displays an icon or an image file.
    Image(gtk::Image, leaf) {
        props {
            icon_name: String,
            file: String,
            resource: String,
            pixel_size: i32,
        }
        signals {}
    }

    /// A horizontal or vertical line.
    Separator(gtk::Separator, leaf) {
        props {
            orientation: gtk::Orientation,
        }
        signals {}
    }

    /// Makes its child scrollable.
    ScrolledWindow(gtk::ScrolledWindow, container) {
        props {
            hscrollbar_policy: gtk::PolicyType,
            vscrollbar_policy: gtk::PolicyType,
            min_content_width: i32,
            min_content_height: i32,
            max_content_width: i32,
            max_content_height: i32,
            propagate_natural_width: bool,
            propagate_natural_height: bool,
            has_frame: bool,
        }
        signals {}
    }

    /// Draws a frame with an optional label around its child.
    Frame(gtk::Frame, container) {
        props {
            label: String,
            label_xalign: Float,
        }
        signals {}
    }

    /// Shows or hides its child when the label is clicked.
    Expander(gtk::Expander, container) {
        props {
            label: String,
            expanded: bool,
            use_markup: bool,
            use_underline: bool,
        }
        signals {
            on_notify_expanded: "notify::expanded",
        }
    }

    /// Shows or hides its child with an animation.
    Revealer(gtk::Revealer, container) {
        props {
            reveal_child: bool,
            transition_type: gtk::RevealerTransitionType,
            transition_duration: u32,
        }
        signals {}
    }

    /// A vertical list. Children are wrapped in rows,
    /// use [ListBoxRow] as the child to configure the row.
    ListBox(gtk::ListBox, container) {
        props {
            selection_mode: gtk::SelectionMode,
            show_separators: bool,
            activate_on_single_click: bool,
        }
        signals {
            on_selected_rows_changed: "selected-rows-changed",
        }
    }

    /// A row of a [ListBox].
    ListBoxRow(gtk::ListBoxRow, container) {
        props {
            activatable: bool,
            selectable: bool,
        }
        signals {
            on_activate: "activate",
        }
    }

    /// Arranges its children in lines that wrap.
    FlowBox(gtk::FlowBox, container) {
        props {
            orientation: gtk::Orientation,
            column_spacing: u32,
            row_spacing: u32,
            min_children_per_line: u32,
            max_children_per_line: u32,
            homogeneous: bool,
            selection_mode: gtk::SelectionMode,
        }
        signals {}
    }
}
