//! GTK4 has no common API for widgets with children, so every kind of container is handled
//! separately here. Children are always the widgets bloom created, even where GTK wraps them
//! (a `gtk::ListBox` puts every child into a `gtk::ListBoxRow`).

use gtk::prelude::*;

enum Container<'a> {
    Box(&'a gtk::Box),
    ListBox(&'a gtk::ListBox),
    FlowBox(&'a gtk::FlowBox),
    /// Any widget with a `child` property, such as windows, buttons, frames or scrolled windows
    Single(&'a gtk::Widget),
}

impl<'a> Container<'a> {
    fn of(parent: &'a gtk::Widget) -> Self {
        if let Some(container) = parent.downcast_ref::<gtk::Box>() {
            Self::Box(container)
        } else if let Some(container) = parent.downcast_ref::<gtk::ListBox>() {
            Self::ListBox(container)
        } else if let Some(container) = parent.downcast_ref::<gtk::FlowBox>() {
            Self::FlowBox(container)
        } else if parent.find_property("child").is_some() {
            Self::Single(parent)
        } else {
            panic!("{} cannot have children", parent.type_().name())
        }
    }
}

/// The widget GTK actually keeps in the container, which may be a wrapper around child
fn slot(parent: &gtk::Widget, child: &gtk::Widget) -> gtk::Widget {
    match child.parent() {
        Some(wrapper) if &wrapper != parent => wrapper,
        _ => child.clone(),
    }
}

/// Insert child into parent before the before-widget, or at the end
pub(crate) fn insert(parent: &gtk::Widget, child: &gtk::Widget, before: Option<&gtk::Widget>) {
    match Container::of(parent) {
        Container::Box(container) => {
            let after = match before {
                Some(before) => before.prev_sibling(),
                None => container.last_child(),
            };
            container.insert_child_after(child, after.as_ref());
        }
        Container::ListBox(container) => {
            let position = before.map_or(-1, |before| {
                slot(parent, before)
                    .downcast::<gtk::ListBoxRow>()
                    .map_or(-1, |row| row.index())
            });
            container.insert(child, position);
        }
        Container::FlowBox(container) => {
            let position = before.map_or(-1, |before| {
                slot(parent, before)
                    .downcast::<gtk::FlowBoxChild>()
                    .map_or(-1, |item| item.index())
            });
            container.insert(child, position);
        }
        Container::Single(container) => {
            let current = container.property::<Option<gtk::Widget>>("child");
            if current.is_some() {
                eprintln!(
                    "bloom-gtk: {} can only have one child, replacing the previous one",
                    container.type_().name()
                );
            }
            container.set_property("child", child);
        }
    }
}

pub(crate) fn remove(parent: &gtk::Widget, child: &gtk::Widget) {
    match Container::of(parent) {
        Container::Box(container) => container.remove(child),
        Container::ListBox(container) => container.remove(&slot(parent, child)),
        Container::FlowBox(container) => container.remove(&slot(parent, child)),
        Container::Single(container) => {
            if container.property::<Option<gtk::Widget>>("child").as_ref() == Some(child) {
                container.set_property("child", None::<gtk::Widget>);
            }
        }
    }
}

/// Move a child that is already in parent before the before-widget, or to the end
pub(crate) fn move_child(parent: &gtk::Widget, child: &gtk::Widget, before: Option<&gtk::Widget>) {
    match Container::of(parent) {
        Container::Box(container) => {
            let after = match before {
                Some(before) => before.prev_sibling(),
                None => container.last_child(),
            };
            if after.as_ref() != Some(child) {
                container.reorder_child_after(child, after.as_ref());
            }
        }
        Container::ListBox(container) => {
            let row = slot(parent, child);
            container.remove(&row);
            insert(parent, &row, before);
        }
        Container::FlowBox(container) => {
            let item = slot(parent, child);
            container.remove(&item);
            insert(parent, &item, before);
        }
        Container::Single(_) => {}
    }
}

/// The children bloom inserted into parent, in order
pub(crate) fn children(parent: &gtk::Widget) -> Vec<gtk::Widget> {
    let unwrap = |slot: gtk::Widget| match slot.downcast_ref::<gtk::ListBoxRow>() {
        Some(row) if !is_own(&slot) => row.child().unwrap_or(slot),
        _ => match slot.downcast_ref::<gtk::FlowBoxChild>() {
            Some(item) if !is_own(&slot) => item.child().unwrap_or(slot),
            _ => slot,
        },
    };
    match Container::of(parent) {
        Container::Single(container) => container
            .property::<Option<gtk::Widget>>("child")
            .into_iter()
            .collect(),
        _ => {
            let mut children = Vec::new();
            let mut next = parent.first_child();
            while let Some(child) = next {
                next = child.next_sibling();
                children.push(unwrap(child));
            }
            children
        }
    }
}

/// Rows and flow box children that bloom created itself (through `<list_box_row>`),
/// as opposed to the wrappers GTK creates around other children.
/// The object model marks every widget it creates.
fn is_own(slot: &gtk::Widget) -> bool {
    unsafe { slot.data::<()>(OWN_KEY).is_some() }
}

const OWN_KEY: &str = "bloom-gtk-own";

/// Mark a widget bloom created, see [is_own]
pub(crate) fn mark_own(widget: &gtk::Widget) {
    unsafe { widget.set_data(OWN_KEY, ()) };
}
