use std::sync::Arc;

use crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::TuiNode;

/// The elements with event handlers in the last frame, in paint order.
#[derive(Default)]
pub(crate) struct Handlers {
    regions: Vec<(Rect, Arc<TuiNode>)>,
}

impl Handlers {
    pub(crate) fn register(&mut self, area: Rect, node: Arc<TuiNode>) {
        self.regions.push((area, node));
    }

    /// Find the event handlers to call for an event.
    /// * `on_key`, `on_paste`, `on_resize` and `on_focus` are called on every element that defines them.
    /// * `on_click`, `on_scroll` and `on_mouse` are only called on the topmost element under the mouse that defines them.
    pub(crate) fn targets(&self, event: &Event) -> Vec<(Arc<TuiNode>, &'static str)> {
        let broadcast = |name: &'static str| {
            self.regions
                .iter()
                .filter(|(_, node)| has_handler(node, name))
                .map(|(_, node)| (node.clone(), name))
                .collect()
        };
        match event {
            Event::Key(_) => broadcast("key"),
            Event::Paste(_) => broadcast("paste"),
            Event::Resize(_, _) => broadcast("resize"),
            Event::FocusGained | Event::FocusLost => broadcast("focus"),
            Event::Mouse(mouse) => {
                let position = Position::new(mouse.column, mouse.row);
                let names: &[&'static str] = match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => &["click", "mouse"],
                    MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight => &["scroll", "mouse"],
                    _ => &["mouse"],
                };
                names
                    .iter()
                    .filter_map(|name| {
                        self.regions
                            .iter()
                            .rev()
                            .find(|(area, node)| area.contains(position) && has_handler(node, name))
                            .map(|(_, node)| (node.clone(), *name))
                    })
                    .collect()
            }
        }
    }
}

fn has_handler(node: &TuiNode, name: &str) -> bool {
    node.as_element()
        .is_some_and(|element| element.callbacks().contains_key(name))
}

/// Call the handlers for an event.
/// The handlers are collected first so no lock is held while they run.
pub(crate) fn dispatch(targets: Vec<(Arc<TuiNode>, &'static str)>, event: &Event) {
    for (node, name) in targets {
        if let Some(handler) = node
            .as_element()
            .and_then(|element| element.callbacks().get(name))
        {
            handler(event.clone());
        }
    }
}
