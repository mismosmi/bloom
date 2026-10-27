//! ObjectModel implementation on top of AppKit via objc2.
use std::{
    fmt::Debug,
    future::{self, Future},
    sync::{Arc, Weak},
};

use bloom_core::{render_loop, Element, ObjectModel};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, sel, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSTextField,
    NSView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};
use taffy::NodeId;
use weak_table::PtrWeakKeyHashMap;

use crate::{
    executor::spawn_main,
    layout::{style_from_attributes, LayoutTree},
    node::{tag, NativeEvent, NativeNode},
};

define_class!(
    /// NSView with a top-left origin, matching taffy's coordinate space.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BloomFlippedView"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

define_class!(
    /// Target for NSControl target/action, forwarding to a Rust closure.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BloomActionTarget"]
    #[ivars = Box<dyn Fn()>]
    struct ActionTarget;

    impl ActionTarget {
        #[unsafe(method(perform:))]
        fn perform(&self, _sender: Option<&AnyObject>) {
            (self.ivars())()
        }
    }
);

impl ActionTarget {
    fn new(mtm: MainThreadMarker, f: Box<dyn Fn()>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(f);
        unsafe { msg_send![super(this), init] }
    }
}

enum Kind {
    Container,
    Button(Retained<NSButton>),
    Label(Retained<NSTextField>),
}

struct NodeState {
    view: Retained<NSView>,
    kind: Kind,
    layout: NodeId,
    /// NSControl holds its target weakly, so we keep it alive here.
    target: Option<Retained<ActionTarget>>,
}

pub struct AppKitModel {
    mtm: MainThreadMarker,
    nodes: PtrWeakKeyHashMap<Weak<NativeNode>, NodeState>,
    layout: LayoutTree,
    root: NodeId,
    root_view: Retained<NSView>,
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

impl AppKitModel {
    fn new(mtm: MainThreadMarker, root: &Arc<NativeNode>, root_view: Retained<NSView>) -> Self {
        let mut layout = LayoutTree::default();
        let root_layout = layout.container(Default::default());
        let mut nodes = PtrWeakKeyHashMap::new();
        nodes.insert(
            root.clone(),
            NodeState {
                view: root_view.clone(),
                kind: Kind::Container,
                layout: root_layout,
                target: None,
            },
        );
        Self {
            mtm,
            nodes,
            layout,
            root: root_layout,
            root_view,
        }
    }

    fn click_target(&self, node: &Arc<NativeNode>) -> Option<Retained<ActionTarget>> {
        let NativeNode::Element(element) = node.as_ref() else {
            return None;
        };
        element.callbacks().get("click")?;
        // hold the node, not the handler: the node is replaced on every update
        let node = node.clone();
        Some(ActionTarget::new(
            self.mtm,
            Box::new(move || {
                if let NativeNode::Element(e) = node.as_ref() {
                    if let Some(cb) = e.callbacks().get("click") {
                        cb(NativeEvent::Click)
                    }
                }
            }),
        ))
    }

    fn build(&mut self, node: &Arc<NativeNode>) -> NodeState {
        let mtm = self.mtm;
        match node.as_ref() {
            NativeNode::Text(text) => {
                let label = NSTextField::labelWithString(&ns(text), mtm);
                let view: Retained<NSView> =
                    Retained::into_super(Retained::into_super(label.clone()));
                let measured = view.clone();
                let layout = self.layout.leaf(
                    Default::default(),
                    Box::new(move |_, _| {
                        let NSSize { width, height } = measured.fittingSize();
                        taffy::Size {
                            width: width as f32,
                            height: height as f32,
                        }
                    }),
                );
                NodeState {
                    view,
                    kind: Kind::Label(label),
                    layout,
                    target: None,
                }
            }
            NativeNode::Element(element) => {
                let style = style_from_attributes(element.tag(), element.attributes());
                match element.tag() {
                    "button" => {
                        let title = element
                            .attributes()
                            .get("title")
                            .cloned()
                            .unwrap_or_default();
                        let target = self.click_target(node);
                        let button = unsafe {
                            NSButton::buttonWithTitle_target_action(
                                &ns(&title),
                                target.as_deref().map(|t| t as &AnyObject),
                                target.as_ref().map(|_| sel!(perform:)),
                                mtm,
                            )
                        };
                        let view: Retained<NSView> =
                            Retained::into_super(Retained::into_super(button.clone()));
                        let measured = view.clone();
                        let layout = self.layout.leaf(
                            style,
                            Box::new(move |_, _| {
                                let NSSize { width, height } = measured.fittingSize();
                                taffy::Size {
                                    width: width as f32,
                                    height: height as f32,
                                }
                            }),
                        );
                        NodeState {
                            view,
                            kind: Kind::Button(button),
                            layout,
                            target,
                        }
                    }
                    // "stack", "view" and unknown tags are plain containers
                    _ => {
                        let view = Retained::into_super(FlippedView::new(mtm));
                        let layout = self.layout.container(style);
                        NodeState {
                            view,
                            kind: Kind::Container,
                            layout,
                            target: None,
                        }
                    }
                }
            }
        }
    }

    fn apply_frames(&self) {
        for (_, state) in self.nodes.iter() {
            if state.layout == self.root {
                continue;
            }
            let (x, y, w, h) = self.layout.frame(state.layout);
            state.view.setFrame(NSRect::new(
                NSPoint::new(x as f64, y as f64),
                NSSize::new(w as f64, h as f64),
            ));
        }
    }
}

impl ObjectModel for AppKitModel {
    type Node = NativeNode;

    fn create(
        &mut self,
        node: &Arc<NativeNode>,
        parent: &Arc<NativeNode>,
        sibling: &Option<Arc<NativeNode>>,
    ) {
        let state = self.build(node);
        let parent_state = self.nodes.get(parent).expect("parent not found");
        let sibling_state = sibling.as_ref().and_then(|s| self.nodes.get(s));
        match sibling_state {
            Some(sibling) => parent_state.view.addSubview_positioned_relativeTo(
                &state.view,
                NSWindowOrderingMode::Below,
                Some(&sibling.view),
            ),
            None => parent_state.view.addSubview(&state.view),
        }
        let (parent_layout, before) = (parent_state.layout, sibling_state.map(|s| s.layout));
        self.layout.insert(parent_layout, state.layout, before);
        self.nodes.insert(node.clone(), state);
    }

    fn remove(&mut self, node: &Arc<NativeNode>, _parent: &Arc<NativeNode>) {
        if let Some(state) = self.nodes.remove(node) {
            state.view.removeFromSuperview();
            self.layout.remove(state.layout);
        }
    }

    fn update(&mut self, node: &Arc<NativeNode>, next: &Arc<NativeNode>) {
        let mut state = self.nodes.remove(node).expect("node not found");
        match (next.as_ref(), &state.kind) {
            (NativeNode::Text(text), Kind::Label(label)) => label.setStringValue(&ns(text)),
            (NativeNode::Element(e), Kind::Button(button)) if e.tag() == "button" => {
                let title = e.attributes().get("title").cloned().unwrap_or_default();
                button.setTitle(&ns(&title));
                state.target = self.click_target(next);
                unsafe { button.setTarget(state.target.as_deref().map(|t| t as &AnyObject)) };
                self.layout
                    .set_style(state.layout, style_from_attributes(e.tag(), e.attributes()));
            }
            (NativeNode::Element(e), Kind::Container) if e.tag() != "button" => {
                self.layout
                    .set_style(state.layout, style_from_attributes(e.tag(), e.attributes()));
            }
            _ => {
                // kind changed: build a replacement in the same slot
                let replacement = self.build(next);
                if let Some(superview) = unsafe { state.view.superview() } {
                    superview.replaceSubview_with(&state.view, &replacement.view);
                }
                // re-parent the layout node; children of a replaced container are
                // re-created by the core since the element type changed
                self.layout.remove(state.layout);
                state = replacement;
            }
        }
        self.nodes.insert(next.clone(), state);
    }

    fn finalize(&mut self) -> impl Future<Output = ()> + Send {
        let size = self.root_view.frame().size;
        self.layout
            .compute(self.root, size.width as f32, size.height as f32);
        self.apply_frames();
        future::ready(())
    }
}

/// Open a window and render `element` into it. Never returns.
pub fn run_app<E>(title: &str, element: Element<NativeNode, E>)
where
    E: Debug + Send + 'static,
{
    let mtm = MainThreadMarker::new().expect("run_app must be called on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(480.0, 320.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Resizable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&ns(title));
    let content: Retained<NSView> = Retained::into_super(FlippedView::new(mtm));
    window.setContentView(Some(&content));
    window.makeKeyAndOrderFront(None);

    let root = Arc::new(NativeNode::Element(Arc::new(tag("window").build())));
    let model = AppKitModel::new(mtm, &root, content);
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
