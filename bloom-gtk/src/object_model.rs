use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Weak},
};

use bloom_core::ObjectModel;
use gtk::{glib, prelude::*};
use weak_table::PtrWeakKeyHashMap;

use crate::{container, node::default_return, GtkNode, NodeKind};

/// The widget rendered for a node
struct Entry {
    widget: gtk::Widget,
    /// The container the widget was inserted into
    parent: Option<gtk::Widget>,
    /// The node currently rendered to the widget. Signal handlers are connected once per signal
    /// and look up the handler of the current node when they are emitted.
    current: Rc<RefCell<Weak<GtkNode>>>,
    signals: HashMap<String, glib::SignalHandlerId>,
}

impl Entry {
    fn create(node: &Arc<GtkNode>, parent: Option<gtk::Widget>) -> Self {
        let widget = match &node.0 {
            NodeKind::Typed(spec) => {
                let widget = spec.create();
                spec.apply(&widget, None);
                widget
            }
            NodeKind::Element(element) => {
                let widget = element.create();
                element.apply(&widget, None);
                widget
            }
            NodeKind::Text(text) => gtk::Label::new(Some(text)).upcast(),
            NodeKind::Root => unreachable!("The root node is never created"),
        };
        container::mark_own(&widget);

        let mut entry = Self {
            widget,
            parent,
            current: Rc::new(RefCell::new(Weak::new())),
            signals: HashMap::new(),
        };
        entry.set_node(node);
        entry
    }

    /// Point the entry to node, connecting and disconnecting signals as needed
    fn set_node(&mut self, node: &Arc<GtkNode>) {
        *self.current.borrow_mut() = Arc::downgrade(node);

        let signals = node.signals();
        let connected: Vec<String> = self.signals.keys().cloned().collect();
        for signal in connected {
            if !signals.contains(&signal.as_str()) {
                if let Some(id) = self.signals.remove(&signal) {
                    self.widget.disconnect(id);
                }
            }
        }
        for signal in signals {
            if !self.signals.contains_key(signal) {
                let current = self.current.clone();
                let name = signal.to_string();
                let return_value = default_return(&self.widget, signal);
                let id = self.widget.connect_local(signal, false, move |args| {
                    let widget = args[0].get::<gtk::Widget>().ok()?;
                    let node = current.borrow().upgrade();
                    if let Some(node) = node {
                        node.emit(&name, &widget);
                    }
                    return_value.clone()
                });
                self.signals.insert(signal.to_string(), id);
            }
        }

        if let Some(widget_ref) = node.widget_ref() {
            widget_ref.set(&self.widget);
        }
    }
}

/// The [ObjectModel] that renders bloom nodes to GTK widgets.
/// It must only be used on the GTK main thread.
pub(crate) struct Gtk {
    nodes: PtrWeakKeyHashMap<Weak<GtkNode>, Entry>,
}

impl Gtk {
    pub(crate) fn new(root: &Arc<GtkNode>, widget: gtk::Widget) -> Self {
        let mut nodes = PtrWeakKeyHashMap::new();
        nodes.insert(
            root.clone(),
            Entry {
                widget,
                parent: None,
                current: Rc::new(RefCell::new(Arc::downgrade(root))),
                signals: HashMap::new(),
            },
        );
        Self { nodes }
    }

    fn widget(&self, node: &Arc<GtkNode>) -> &gtk::Widget {
        &self.nodes.get(node).expect("Node not found").widget
    }
}

impl ObjectModel for Gtk {
    type Node = GtkNode;

    fn create(
        &mut self,
        node: &Arc<GtkNode>,
        parent: &Arc<GtkNode>,
        sibling: &Option<Arc<GtkNode>>,
    ) {
        let parent = self.widget(parent).clone();
        let before = sibling.as_ref().map(|sibling| self.widget(sibling));
        let entry = Entry::create(node, Some(parent.clone()));
        container::insert(&parent, &entry.widget, before);
        self.nodes.insert(node.clone(), entry);
    }

    fn remove(&mut self, node: &Arc<GtkNode>, _parent: &Arc<GtkNode>) {
        let entry = self.nodes.remove(node).expect("Node not found");
        // A single-child container may already have replaced the widget
        if let Some(parent) = &entry.parent {
            if entry.widget.is_ancestor(parent) {
                container::remove(parent, &entry.widget);
            }
        }
        for (_, id) in entry.signals {
            entry.widget.disconnect(id);
        }
    }

    fn move_node(
        &mut self,
        node: &Arc<GtkNode>,
        parent: &Arc<GtkNode>,
        sibling: &Option<Arc<GtkNode>>,
    ) {
        let parent = self.widget(parent);
        let before = sibling.as_ref().map(|sibling| self.widget(sibling));
        container::move_child(parent, self.widget(node), before);
    }

    fn update(&mut self, node: &Arc<GtkNode>, next: &Arc<GtkNode>) {
        let mut entry = self.nodes.remove(node).expect("Node not found");

        if !node.same_kind(next) {
            // bloom-core updates the children of the node after this, so they are moved
            // from the old widget to the new one
            let replacement = Entry::create(next, entry.parent.clone());
            for child in container::children(&entry.widget) {
                container::remove(&entry.widget, &child);
                container::insert(&replacement.widget, &child, None);
            }
            if let Some(parent) = &entry.parent {
                container::insert(parent, &replacement.widget, Some(&entry.widget));
                container::remove(parent, &entry.widget);
            }
            for (_, id) in entry.signals.drain() {
                entry.widget.disconnect(id);
            }
            self.nodes.insert(next.clone(), replacement);
            return;
        }

        match (&node.0, &next.0) {
            (NodeKind::Typed(prev), NodeKind::Typed(spec)) => {
                spec.apply(&entry.widget, Some(prev.as_ref()))
            }
            (NodeKind::Element(prev), NodeKind::Element(element)) => {
                element.apply(&entry.widget, Some(prev))
            }
            (NodeKind::Text(prev), NodeKind::Text(text)) => {
                if prev != text {
                    entry.widget.set_property("label", text);
                }
            }
            _ => unreachable!("Nodes of the same kind"),
        }
        entry.set_node(next);
        self.nodes.insert(next.clone(), entry);
    }
}
