//! Node types mirroring AppKit classes. Each node is a typed description of one
//! AppKit object; props are named after the AppKit property they set and only
//! touch it when given, so unset props keep AppKit's own defaults.
//!
//! The uppercase names make `rsx!` treat them like components:
//! `<Button title="OK" />` expands to `Button::builder().title("OK").build().into()`.
use std::sync::{Arc, Mutex};

use bloom_core::Element;
use dispatch2::MainThreadBound;
use objc2::{rc::Retained, MainThreadMarker, Message};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSLayoutAttribute, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSWindow,
};
use objc2_foundation::{NSEdgeInsets, NSSize};

/// An action handler. AppKit passes the sending control, so do we.
pub type Action<T> = Arc<dyn Fn(&T) + Send + Sync + 'static>;

/// The bloom equivalent of an IBOutlet: obtain one with `use_ref::<Outlet<NSButton>>()`,
/// pass it as `outlet={...}` and read it in an effect.
pub struct Outlet<T: 'static>(Mutex<Option<MainThreadBound<Retained<T>>>>);

impl<T> Default for Outlet<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<T: Message> Outlet<T> {
    pub fn get(&self, mtm: MainThreadMarker) -> Option<Retained<T>> {
        let slot = self.0.lock().unwrap();
        slot.as_ref().map(|bound| Retained::clone(bound.get(mtm)))
    }

    pub(crate) fn set(&self, mtm: MainThreadMarker, object: Retained<T>) {
        *self.0.lock().unwrap() = Some(MainThreadBound::new(object, mtm));
    }
}

impl<T> std::hash::Hash for Outlet<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self, state)
    }
}

impl<T> PartialEq for Outlet<T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

pub enum AppKitNode {
    /// The root, standing for NSApplication. Its children must be windows.
    Application,
    Window(Window),
    StackView(StackView),
    Button(Button),
    TextField(TextField),
}

/// Bare strings in rsx become labels, like `+[NSTextField labelWithString:]`.
impl From<String> for AppKitNode {
    fn from(value: String) -> Self {
        AppKitNode::TextField(TextField {
            string_value: Some(value),
            editable: Some(false),
            selectable: Some(false),
            bezeled: Some(false),
            draws_background: Some(false),
            ..Default::default()
        })
    }
}

/// Setters for props whose value converts into the stored type.
macro_rules! props {
    ($($name:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(mut self, value: impl Into<$ty>) -> Self {
                self.$name = Some(value.into());
                self
            }
        )*
    };
}

macro_rules! actions {
    ($target:ty; $($name:ident),* $(,)?) => {
        $(
            pub fn $name(mut self, action: impl Fn(&$target) + Send + Sync + 'static) -> Self {
                self.$name = Some(Arc::new(action));
                self
            }
        )*
    };
}

/// A builder for nodes that take children.
pub struct WithChildren<P, E> {
    props: P,
    children: Vec<Element<AppKitNode, E>>,
}

impl<P, E> WithChildren<P, E> {
    pub fn children(mut self, children: Vec<Element<AppKitNode, E>>) -> Self {
        self.children = children;
        self
    }
}

/// `NSWindow`. Its single child becomes the `contentView`.
#[derive(Default)]
pub struct Window {
    pub title: Option<String>,
    pub content_size: Option<NSSize>,
    pub outlet: Option<Arc<Outlet<NSWindow>>>,
}

impl Window {
    pub fn builder<E>() -> WithChildren<Window, E> {
        WithChildren {
            props: Window::default(),
            children: Vec::new(),
        }
    }
}

impl<E> WithChildren<Window, E> {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.props.title = Some(title.into());
        self
    }
    pub fn content_size(mut self, size: NSSize) -> Self {
        self.props.content_size = Some(size);
        self
    }
    pub fn outlet(mut self, outlet: Arc<Outlet<NSWindow>>) -> Self {
        self.props.outlet = Some(outlet);
        self
    }
    pub fn build(self) -> Element<AppKitNode, E> {
        Element::Node(AppKitNode::Window(self.props), self.children)
    }
}

/// `NSStackView`, AppKit's layout container. Children become arranged subviews.
#[derive(Default)]
pub struct StackView {
    pub orientation: Option<NSUserInterfaceLayoutOrientation>,
    pub alignment: Option<NSLayoutAttribute>,
    pub spacing: Option<f64>,
    pub edge_insets: Option<NSEdgeInsets>,
    pub outlet: Option<Arc<Outlet<NSStackView>>>,
}

impl StackView {
    pub fn builder<E>() -> WithChildren<StackView, E> {
        WithChildren {
            props: StackView::default(),
            children: Vec::new(),
        }
    }
}

impl<E> WithChildren<StackView, E> {
    pub fn orientation(mut self, orientation: NSUserInterfaceLayoutOrientation) -> Self {
        self.props.orientation = Some(orientation);
        self
    }
    pub fn alignment(mut self, alignment: NSLayoutAttribute) -> Self {
        self.props.alignment = Some(alignment);
        self
    }
    pub fn spacing(mut self, spacing: impl Into<f64>) -> Self {
        self.props.spacing = Some(spacing.into());
        self
    }
    pub fn edge_insets(mut self, insets: NSEdgeInsets) -> Self {
        self.props.edge_insets = Some(insets);
        self
    }
    pub fn outlet(mut self, outlet: Arc<Outlet<NSStackView>>) -> Self {
        self.props.outlet = Some(outlet);
        self
    }
    pub fn build(self) -> Element<AppKitNode, E> {
        Element::Node(AppKitNode::StackView(self.props), self.children)
    }
}

/// `NSButton`. `on_action` is its target/action pair.
#[derive(Default)]
pub struct Button {
    pub title: Option<String>,
    pub enabled: Option<bool>,
    pub bezel_style: Option<NSBezelStyle>,
    pub key_equivalent: Option<String>,
    pub on_action: Option<Action<NSButton>>,
    pub outlet: Option<Arc<Outlet<NSButton>>>,
}

impl Button {
    pub fn builder() -> Self {
        Self::default()
    }
    props!(title: String, enabled: bool, bezel_style: NSBezelStyle, key_equivalent: String);
    actions!(NSButton; on_action);
    pub fn outlet(mut self, outlet: Arc<Outlet<NSButton>>) -> Self {
        self.outlet = Some(outlet);
        self
    }
    pub fn build(self) -> Self {
        self
    }
}

/// `NSTextField`. `on_action` fires on Return, `on_change` is the delegate's
/// `controlTextDidChange:`.
#[derive(Default)]
pub struct TextField {
    pub string_value: Option<String>,
    pub placeholder_string: Option<String>,
    pub editable: Option<bool>,
    pub selectable: Option<bool>,
    pub bezeled: Option<bool>,
    pub draws_background: Option<bool>,
    pub on_action: Option<Action<NSTextField>>,
    pub on_change: Option<Action<NSTextField>>,
    pub outlet: Option<Arc<Outlet<NSTextField>>>,
}

impl TextField {
    pub fn builder() -> Self {
        Self::default()
    }
    props!(
        string_value: String,
        placeholder_string: String,
        editable: bool,
        selectable: bool,
        bezeled: bool,
        draws_background: bool,
    );
    actions!(NSTextField; on_action, on_change);
    pub fn outlet(mut self, outlet: Arc<Outlet<NSTextField>>) -> Self {
        self.outlet = Some(outlet);
        self
    }
    pub fn build(self) -> Self {
        self
    }
}

impl<E> From<Button> for Element<AppKitNode, E> {
    fn from(button: Button) -> Self {
        Element::Node(AppKitNode::Button(button), Vec::new())
    }
}

impl<E> From<TextField> for Element<AppKitNode, E> {
    fn from(text_field: TextField) -> Self {
        Element::Node(AppKitNode::TextField(text_field), Vec::new())
    }
}
