//! Flexbox layout via taffy. AppKit has no flexbox, so the renderer owns layout
//! and assigns explicit frames (the React Native / Yoga approach).
use std::collections::HashMap;

use taffy::{prelude::*, LayoutOutput};

/// Leaf measurement is delegated to the platform (e.g. NSTextField fittingSize).
pub type Measure = Box<dyn Fn(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>>;

pub struct LayoutTree {
    taffy: TaffyTree<Measure>,
}

fn px(value: &str) -> Option<f32> {
    value.trim_end_matches("px").parse().ok()
}

/// Translate stringly rsx attributes into a taffy style.
pub fn style_from_attributes(tag: &str, attrs: &HashMap<String, String>) -> Style {
    let mut style = Style::default();
    if tag == "stack" {
        style.display = Display::Flex;
        style.flex_direction = match attrs.get("direction").map(String::as_str) {
            Some("row") => FlexDirection::Row,
            _ => FlexDirection::Column,
        };
    }
    if let Some(gap) = attrs.get("gap").and_then(|v| px(v)) {
        style.gap = Size {
            width: length(gap),
            height: length(gap),
        };
    }
    if let Some(p) = attrs.get("padding").and_then(|v| px(v)) {
        style.padding = Rect {
            left: length(p),
            right: length(p),
            top: length(p),
            bottom: length(p),
        };
    }
    if let Some(w) = attrs.get("width").and_then(|v| px(v)) {
        style.size.width = length(w);
    }
    if let Some(h) = attrs.get("height").and_then(|v| px(v)) {
        style.size.height = length(h);
    }
    if let Some(g) = attrs.get("grow").and_then(|v| px(v)) {
        style.flex_grow = g;
    }
    style
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self {
            taffy: TaffyTree::new(),
        }
    }
}

impl LayoutTree {
    pub fn container(&mut self, style: Style) -> NodeId {
        self.taffy.new_leaf(style).expect("taffy: new_leaf")
    }

    pub fn leaf(&mut self, style: Style, measure: Measure) -> NodeId {
        self.taffy
            .new_leaf_with_context(style, measure)
            .expect("taffy: new_leaf_with_context")
    }

    pub fn set_style(&mut self, node: NodeId, style: Style) {
        self.taffy.set_style(node, style).expect("taffy: set_style");
    }

    /// Insert `child` into `parent` before `before`, or append.
    pub fn insert(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        let index = before
            .and_then(|b| {
                self.taffy
                    .children(parent)
                    .ok()?
                    .iter()
                    .position(|c| *c == b)
            })
            .unwrap_or_else(|| self.taffy.child_count(parent));
        self.taffy
            .insert_child_at_index(parent, index, child)
            .expect("taffy: insert_child_at_index");
    }

    pub fn remove(&mut self, node: NodeId) {
        let _ = self.taffy.remove(node);
    }

    pub fn compute(&mut self, root: NodeId, width: f32, height: f32) {
        self.taffy
            .compute_layout_with_measure(
                root,
                Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::Definite(height),
                },
                |input, _node, measure, _style| {
                    LayoutOutput::from_outer_size(match measure {
                        Some(measure) => measure(input.known_dimensions, input.available_space),
                        None => Size::ZERO,
                    })
                },
            )
            .expect("taffy: compute_layout");
    }

    /// Frame relative to the parent, in a top-left-origin coordinate space.
    pub fn frame(&self, node: NodeId) -> (f32, f32, f32, f32) {
        let l = self.taffy.layout(node).expect("taffy: layout");
        (l.location.x, l.location.y, l.size.width, l.size.height)
    }
}
