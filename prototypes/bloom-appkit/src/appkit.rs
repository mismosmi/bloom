//! `ObjectModel` over AppKit. Layout is left entirely to AppKit: windows own a
//! content view and `NSStackView` arranges its subviews with Auto Layout.
use std::{
    cell::RefCell,
    fmt::Debug,
    sync::{Arc, Weak},
};

use bloom_core::{render_loop, Element, ObjectModel};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::NSObjectProtocol,
    runtime::ProtocolObject, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton,
    NSControlTextEditingDelegate, NSStackView, NSTextField, NSTextFieldDelegate, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString};
use weak_table::PtrWeakKeyHashMap;

use crate::{
    executor::spawn_main,
    node::{Action, AppKitNode, Button, StackView, TextField, Window},
};

type Handler = Box<dyn Fn(&AnyObject)>;

#[derive(Default)]
struct Handlers {
    action: RefCell<Option<Handler>>,
    text_change: RefCell<Option<Handler>>,
}

define_class!(
    /// Target for target/action and delegate for text fields. Controls hold
    /// both weakly, so the model keeps this alive next to the control.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BloomTarget"]
    #[ivars = Handlers]
    struct BloomTarget;

    impl BloomTarget {
        #[unsafe(method(performAction:))]
        fn perform_action(&self, sender: &AnyObject) {
            if let Some(handler) = &*self.ivars().action.borrow() {
                handler(sender)
            }
        }
    }

    unsafe impl NSObjectProtocol for BloomTarget {}

    unsafe impl NSControlTextEditingDelegate for BloomTarget {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            if let (Some(handler), Some(sender)) =
                (&*self.ivars().text_change.borrow(), notification.object())
            {
                handler(&sender)
            }
        }
    }

    unsafe impl NSTextFieldDelegate for BloomTarget {}
);

impl BloomTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Handlers::default());
        unsafe { msg_send![super(this), init] }
    }

    fn set_action<T: objc2::DowncastTarget>(&self, action: &Option<Action<T>>) {
        *self.ivars().action.borrow_mut() = action.clone().map(|action| -> Handler {
            Box::new(move |sender| {
                if let Some(sender) = sender.downcast_ref::<T>() {
                    action(sender)
                }
            })
        });
    }

    fn set_text_change(&self, action: &Option<Action<NSTextField>>) {
        *self.ivars().text_change.borrow_mut() = action.clone().map(|action| -> Handler {
            Box::new(move |sender| {
                if let Some(sender) = sender.downcast_ref::<NSTextField>() {
                    action(sender)
                }
            })
        });
    }
}

enum Native {
    Application,
    Window(Retained<NSWindow>),
    StackView(Retained<NSStackView>),
    Button(Retained<NSButton>, Retained<BloomTarget>),
    TextField(Retained<NSTextField>, Retained<BloomTarget>),
}

impl Native {
    fn view(&self) -> Option<&NSView> {
        match self {
            Native::Application | Native::Window(_) => None,
            Native::StackView(view) => Some(view),
            Native::Button(view, _) => Some(view),
            Native::TextField(view, _) => Some(view),
        }
    }
}

/// Where a node's object is attached, so it can be detached or swapped later.
enum Slot {
    Application,
    ContentView(Retained<NSWindow>),
    Arranged(Retained<NSStackView>),
}

struct NodeState {
    native: Native,
    slot: Slot,
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

/// The prop's new value when it is set and differs from the old node's.
fn changed<'a, T: PartialEq>(old: Option<&Option<T>>, new: &'a Option<T>) -> Option<&'a T> {
    match (old, new) {
        (Some(Some(old)), Some(new)) if old == new => None,
        (_, new) => new.as_ref(),
    }
}

fn apply_window(window: &NSWindow, old: Option<&Window>, new: &Window, mtm: MainThreadMarker) {
    if let Some(title) = changed(old.map(|o| &o.title), &new.title) {
        window.setTitle(&ns(title));
    }
    if let Some(size) = changed(old.map(|o| &o.content_size), &new.content_size) {
        window.setContentSize(*size);
    }
    if let Some(outlet) = &new.outlet {
        outlet.set(mtm, window.retain());
    }
}

fn apply_stack_view(
    stack: &NSStackView,
    old: Option<&StackView>,
    new: &StackView,
    mtm: MainThreadMarker,
) {
    if let Some(orientation) = changed(old.map(|o| &o.orientation), &new.orientation) {
        stack.setOrientation(*orientation);
    }
    if let Some(alignment) = changed(old.map(|o| &o.alignment), &new.alignment) {
        stack.setAlignment(*alignment);
    }
    if let Some(spacing) = changed(old.map(|o| &o.spacing), &new.spacing) {
        stack.setSpacing(*spacing);
    }
    if let Some(insets) = changed(old.map(|o| &o.edge_insets), &new.edge_insets) {
        stack.setEdgeInsets(*insets);
    }
    if let Some(outlet) = &new.outlet {
        outlet.set(mtm, stack.retain());
    }
}

fn apply_button(
    button: &NSButton,
    target: &BloomTarget,
    old: Option<&Button>,
    new: &Button,
    mtm: MainThreadMarker,
) {
    if let Some(title) = changed(old.map(|o| &o.title), &new.title) {
        button.setTitle(&ns(title));
    }
    if let Some(enabled) = changed(old.map(|o| &o.enabled), &new.enabled) {
        button.setEnabled(*enabled);
    }
    if let Some(style) = changed(old.map(|o| &o.bezel_style), &new.bezel_style) {
        button.setBezelStyle(*style);
    }
    if let Some(key) = changed(old.map(|o| &o.key_equivalent), &new.key_equivalent) {
        button.setKeyEquivalent(&ns(key));
    }
    // closures can't be compared, so the handler is rebound on every pass
    target.set_action(&new.on_action);
    if let Some(outlet) = &new.outlet {
        outlet.set(mtm, button.retain());
    }
}

fn apply_text_field(
    field: &NSTextField,
    target: &BloomTarget,
    old: Option<&TextField>,
    new: &TextField,
    mtm: MainThreadMarker,
) {
    // compare against the field itself, not the old props: the user may have
    // typed since, and rewriting an equal value would move the cursor
    if let Some(value) = &new.string_value {
        if field.stringValue().to_string() != *value {
            field.setStringValue(&ns(value));
        }
    }
    if let Some(placeholder) = changed(old.map(|o| &o.placeholder_string), &new.placeholder_string)
    {
        field.setPlaceholderString(Some(&ns(placeholder)));
    }
    if let Some(editable) = changed(old.map(|o| &o.editable), &new.editable) {
        field.setEditable(*editable);
    }
    if let Some(selectable) = changed(old.map(|o| &o.selectable), &new.selectable) {
        field.setSelectable(*selectable);
    }
    if let Some(bezeled) = changed(old.map(|o| &o.bezeled), &new.bezeled) {
        field.setBezeled(*bezeled);
    }
    if let Some(draws) = changed(old.map(|o| &o.draws_background), &new.draws_background) {
        field.setDrawsBackground(*draws);
    }
    target.set_action(&new.on_action);
    target.set_text_change(&new.on_change);
    if let Some(outlet) = &new.outlet {
        outlet.set(mtm, field.retain());
    }
}

pub struct AppKitModel {
    mtm: MainThreadMarker,
    nodes: PtrWeakKeyHashMap<Weak<AppKitNode>, NodeState>,
}

impl AppKitModel {
    fn new(mtm: MainThreadMarker, root: &Arc<AppKitNode>) -> Self {
        let mut nodes = PtrWeakKeyHashMap::new();
        nodes.insert(
            root.clone(),
            NodeState {
                native: Native::Application,
                slot: Slot::Application,
            },
        );
        Self { mtm, nodes }
    }

    fn build(&self, node: &AppKitNode) -> Native {
        let mtm = self.mtm;
        match node {
            AppKitNode::Application => panic!("Application can only be the root"),
            AppKitNode::Window(props) => {
                let size = props.content_size.unwrap_or(NSSize::new(480.0, 320.0));
                let window = unsafe {
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        NSWindow::alloc(mtm),
                        NSRect::new(NSPoint::new(0.0, 0.0), size),
                        NSWindowStyleMask::Titled
                            | NSWindowStyleMask::Closable
                            | NSWindowStyleMask::Miniaturizable
                            | NSWindowStyleMask::Resizable,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                };
                unsafe { window.setReleasedWhenClosed(false) };
                apply_window(&window, None, props, mtm);
                window.center();
                window.makeKeyAndOrderFront(None);
                Native::Window(window)
            }
            AppKitNode::StackView(props) => {
                let stack = NSStackView::new(mtm);
                apply_stack_view(&stack, None, props, mtm);
                Native::StackView(stack)
            }
            AppKitNode::Button(props) => {
                let target = BloomTarget::new(mtm);
                let button = NSButton::new(mtm);
                button.setBezelStyle(objc2_app_kit::NSBezelStyle::Push);
                unsafe {
                    button.setTarget(Some(&target));
                    button.setAction(Some(sel!(performAction:)));
                }
                apply_button(&button, &target, None, props, mtm);
                Native::Button(button, target)
            }
            AppKitNode::TextField(props) => {
                let target = BloomTarget::new(mtm);
                let field = NSTextField::new(mtm);
                unsafe {
                    field.setTarget(Some(&target));
                    field.setAction(Some(sel!(performAction:)));
                }
                unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*target))) };

                apply_text_field(&field, &target, None, props, mtm);
                Native::TextField(field, target)
            }
        }
    }

    /// Attach `native` under `parent`, before `sibling`'s object if given.
    fn attach(&self, native: &Native, parent: &Native, sibling: Option<&Native>) -> Slot {
        match (parent, native.view()) {
            (Native::Application, None) => Slot::Application,
            (Native::Window(window), Some(view)) => {
                window.setContentView(Some(view));
                Slot::ContentView(window.clone())
            }
            (Native::StackView(stack), Some(view)) => {
                let index = sibling.and_then(Native::view).and_then(|sibling| {
                    stack.arrangedSubviews().iter().position(|v| &*v == sibling)
                });
                match index {
                    Some(index) => stack.insertArrangedSubview_atIndex(view, index as isize),
                    None => stack.addArrangedSubview(view),
                }
                Slot::Arranged(stack.clone())
            }
            (Native::Application, Some(_)) => panic!("children of the application must be windows"),
            (_, None) => panic!("windows must be children of the application"),
            _ => panic!("buttons and text fields cannot have children"),
        }
    }

    fn detach(native: &Native, slot: &Slot) {
        match (native, slot) {
            (Native::Window(window), _) => window.close(),
            (native, Slot::ContentView(window)) => {
                if native.view().is_some() {
                    window.setContentView(None);
                }
            }
            (native, Slot::Arranged(stack)) => {
                if let Some(view) = native.view() {
                    stack.removeArrangedSubview(view);
                    view.removeFromSuperview();
                }
            }
            _ => {}
        }
    }

    /// Put `replacement` where `current` is, for a node whose AppKit class changed.
    fn swap(current: &Native, replacement: &Native, slot: &Slot) {
        match (slot, current.view(), replacement.view()) {
            (Slot::ContentView(window), _, Some(view)) => window.setContentView(Some(view)),
            (Slot::Arranged(stack), Some(old), Some(new)) => {
                let index = stack.arrangedSubviews().iter().position(|v| &*v == old);
                match index {
                    Some(index) => stack.insertArrangedSubview_atIndex(new, index as isize),
                    None => stack.addArrangedSubview(new),
                }
                stack.removeArrangedSubview(old);
                old.removeFromSuperview();
            }
            _ => panic!("cannot replace a window with a view or vice versa"),
        }
    }
}

impl ObjectModel for AppKitModel {
    type Node = AppKitNode;

    fn create(
        &mut self,
        node: &Arc<AppKitNode>,
        parent: &Arc<AppKitNode>,
        sibling: &Option<Arc<AppKitNode>>,
    ) {
        let native = self.build(node);
        let parent = &self.nodes.get(parent).expect("parent not found").native;
        let sibling = sibling
            .as_ref()
            .and_then(|s| self.nodes.get(s))
            .map(|s| &s.native);
        let slot = self.attach(&native, parent, sibling);
        self.nodes.insert(node.clone(), NodeState { native, slot });
    }

    fn remove(&mut self, node: &Arc<AppKitNode>, _parent: &Arc<AppKitNode>) {
        if let Some(state) = self.nodes.remove(node) {
            Self::detach(&state.native, &state.slot);
        }
    }

    fn update(&mut self, node: &Arc<AppKitNode>, next: &Arc<AppKitNode>) {
        let mtm = self.mtm;
        let mut state = self.nodes.remove(node).expect("node not found");
        match (&state.native, node.as_ref(), next.as_ref()) {
            (Native::Window(w), AppKitNode::Window(old), AppKitNode::Window(new)) => {
                apply_window(w, Some(old), new, mtm)
            }
            (Native::StackView(s), AppKitNode::StackView(old), AppKitNode::StackView(new)) => {
                apply_stack_view(s, Some(old), new, mtm)
            }
            (Native::Button(b, t), AppKitNode::Button(old), AppKitNode::Button(new)) => {
                apply_button(b, t, Some(old), new, mtm)
            }
            (Native::TextField(f, t), AppKitNode::TextField(old), AppKitNode::TextField(new)) => {
                apply_text_field(f, t, Some(old), new, mtm)
            }
            _ => {
                // The AppKit class changed. Leaves swap cleanly; a replaced stack
                // view loses its children until bloom-core treats this as
                // remove + create (see docs/macos-renderer.md, §3.1).
                let replacement = self.build(next);
                Self::swap(&state.native, &replacement, &state.slot);
                state.native = replacement;
            }
        }
        self.nodes.insert(next.clone(), state);
    }
}

/// Run an app whose root element renders `Window`s. Never returns.
pub fn run<E>(element: Element<AppKitNode, E>)
where
    E: Debug + Send + 'static,
{
    let mtm = MainThreadMarker::new().expect("run must be called on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let root = Arc::new(AppKitNode::Application);
    let model = AppKitModel::new(mtm, &root);
    let pool = futures_executor::ThreadPool::new().expect("thread pool");

    spawn_main(mtm, async move {
        if let Err(error) = render_loop(root, element, pool, model).await {
            eprintln!("render loop error: {error:?}");
        }
    });

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    app.run();
}
