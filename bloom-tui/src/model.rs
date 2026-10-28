use std::{
    any::{Any, TypeId},
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};

use async_channel::Sender;
use bloom_core::ObjectModel;
use futures_util::{future, Future};
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Widget},
    Terminal,
};
use taffy::{AvailableSpace, Dimension, NodeId, TaffyTree};

use crate::{
    events::Handlers,
    style::{Align, ElementStyle, Kind, Wrap},
    text::Paragraph,
    TuiNode,
};

/// Nodes are identified by the address of the `Arc` bloom-core holds them in.
type Key = usize;

fn key(node: &Arc<TuiNode>) -> Key {
    Arc::as_ptr(node) as Key
}

struct NodeState {
    node: Arc<TuiNode>,
    parent: Option<Key>,
    children: Vec<Key>,
    style: Option<ElementStyle>,
}

impl NodeState {
    fn new(node: &Arc<TuiNode>, parent: Option<Key>) -> Self {
        Self {
            node: node.clone(),
            parent,
            children: Vec::new(),
            style: parse_style(node),
        }
    }
}

fn parse_style(node: &Arc<TuiNode>) -> Option<ElementStyle> {
    node.as_element()
        .map(|element| ElementStyle::parse(element.tag_name(), element.attributes()))
}

/// Writes the escape sequences that make the terminal show a frame atomically,
/// so partially drawn frames never become visible.
pub(crate) trait SynchronizedOutput {
    fn begin_frame(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn end_frame(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SynchronizedOutput for ratatui::backend::TestBackend {}

impl<W: io::Write> SynchronizedOutput for ratatui::backend::CrosstermBackend<W> {
    fn begin_frame(&mut self) -> io::Result<()> {
        crossterm::queue!(self, crossterm::terminal::BeginSynchronizedUpdate)
    }
    fn end_frame(&mut self) -> io::Result<()> {
        crossterm::execute!(self, crossterm::terminal::EndSynchronizedUpdate)
    }
}

/// Everything the renderer shares with the event loop
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) handlers: Handlers,
    pub(crate) signal: Option<Sender<()>>,
    pub(crate) error: Option<io::Error>,
}

pub(crate) type SharedState = Arc<Mutex<Shared>>;

type FrameCallback = Box<dyn Fn(&Buffer) + Send>;

/// The bloom ObjectModel for terminals.
/// It keeps a retained copy of the node tree,
/// lays it out with taffy and paints it into a ratatui buffer,
/// ratatui then only writes the cells that changed since the last frame.
pub(crate) struct TuiModel<B: Backend> {
    terminal: Terminal<B>,
    nodes: HashMap<Key, NodeState>,
    root: Key,
    shared: SharedState,
    context: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    on_frame: Option<FrameCallback>,
}

impl<B> TuiModel<B>
where
    B: Backend + SynchronizedOutput,
{
    pub(crate) fn new(
        terminal: Terminal<B>,
        root: &Arc<TuiNode>,
        shared: SharedState,
        context: Vec<Arc<dyn Any + Send + Sync>>,
    ) -> Self {
        let mut nodes = HashMap::new();
        nodes.insert(key(root), NodeState::new(root, None));
        Self {
            terminal,
            nodes,
            root: key(root),
            shared,
            context: Arc::new(
                context
                    .into_iter()
                    .map(|value| ((*value).type_id(), value))
                    .collect(),
            ),
            on_frame: None,
        }
    }

    pub(crate) fn on_frame(mut self, callback: impl Fn(&Buffer) + Send + 'static) -> Self {
        self.on_frame = Some(Box::new(callback));
        self
    }

    fn draw(&mut self) -> io::Result<()>
    where
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        self.terminal.autoresize().map_err(io::Error::other)?;
        let area = self.terminal.get_frame().area();
        let mut handlers = Handlers::default();
        let layout = self.layout(area);

        self.terminal.backend_mut().begin_frame()?;
        let frame = self
            .terminal
            .draw(|frame| {
                if let Some(layout) = &layout {
                    let mut painter = Painter {
                        nodes: &self.nodes,
                        layout,
                        buffer: frame.buffer_mut(),
                        handlers: &mut handlers,
                    };
                    painter.paint(
                        self.root,
                        layout.root,
                        area.x as i32,
                        area.y as i32,
                        area,
                        Style::default(),
                    );
                }
            })
            .map_err(io::Error::other)?;
        if let Some(on_frame) = &self.on_frame {
            on_frame(frame.buffer);
        }
        self.terminal.backend_mut().end_frame()?;

        self.shared.lock().expect("TUI state poisoned").handlers = handlers;
        Ok(())
    }

    /// Build a taffy tree from the retained node tree and compute its layout.
    fn layout(&self, area: Rect) -> Option<Layout> {
        let mut layout = Layout {
            taffy: TaffyTree::new(),
            keys: HashMap::new(),
            root: NodeId::from(0u64),
        };
        let root = self.nodes.get(&self.root)?;
        let mut root_style = root.style.clone()?.layout.0;
        root_style.size = taffy::Size {
            width: Dimension::length(area.width as f32),
            height: Dimension::length(area.height as f32),
        };
        let children = self.build_children(root, &mut layout);
        layout.root = layout.taffy.new_with_children(root_style, &children).ok()?;
        layout.keys.insert(layout.root, self.root);

        layout
            .taffy
            .compute_layout_with_measure(
                layout.root,
                taffy::Size {
                    width: AvailableSpace::Definite(area.width as f32),
                    height: AvailableSpace::Definite(area.height as f32),
                },
                |inputs, _node_id, paragraph, style| {
                    taffy::compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, available| match paragraph {
                            Some(paragraph) => measure(paragraph, known, available),
                            None => taffy::Size::ZERO,
                        },
                    )
                },
            )
            .ok()?;
        Some(layout)
    }

    fn build_children(&self, state: &NodeState, layout: &mut Layout) -> Vec<NodeId> {
        state
            .children
            .iter()
            .filter_map(|child| self.build(*child, layout))
            .collect()
    }

    fn build(&self, key: Key, layout: &mut Layout) -> Option<NodeId> {
        let state = self.nodes.get(&key)?;
        let id = match (state.node.as_ref(), &state.style) {
            (TuiNode::Text(text), _) => layout
                .taffy
                .new_leaf_with_context(
                    taffy::Style {
                        flex_shrink: 1.0,
                        ..Default::default()
                    },
                    Paragraph {
                        spans: vec![(text.clone(), Style::default())],
                        wrap: Wrap::Word,
                    },
                )
                .ok()?,
            (TuiNode::Element(_), Some(style)) if style.kind == Kind::Text => {
                let mut paragraph = Paragraph {
                    spans: Vec::new(),
                    wrap: style.wrap,
                };
                self.collect_spans(key, Style::default(), &mut paragraph.spans);
                layout
                    .taffy
                    .new_leaf_with_context(style.layout.0.clone(), paragraph)
                    .ok()?
            }
            (TuiNode::Element(_), Some(style)) => {
                let children = self.build_children(state, layout);
                layout
                    .taffy
                    .new_with_children(style.layout.0.clone(), &children)
                    .ok()?
            }
            (TuiNode::Element(_), None) => return None,
        };
        layout.keys.insert(id, key);
        Some(id)
    }

    /// Flatten the text inside a `<text>` element into styled spans.
    /// The style of the `<text>` element itself is applied when painting.
    fn collect_spans(&self, key: Key, style: Style, spans: &mut Vec<(String, Style)>) {
        let Some(state) = self.nodes.get(&key) else {
            return;
        };
        for child in &state.children {
            let Some(child_state) = self.nodes.get(child) else {
                continue;
            };
            match (child_state.node.as_ref(), &child_state.style) {
                (TuiNode::Text(text), _) => spans.push((text.clone(), style)),
                (TuiNode::Element(_), Some(child_style)) => {
                    if child_style.layout.display != taffy::Display::None {
                        self.collect_spans(*child, style.patch(child_style.text), spans)
                    }
                }
                (TuiNode::Element(_), None) => {}
            }
        }
    }

    fn insert_child(&mut self, parent: Key, child: Key, sibling: Option<Key>) {
        if let Some(parent_state) = self.nodes.get_mut(&parent) {
            let index = sibling
                .and_then(|sibling| parent_state.children.iter().position(|c| *c == sibling))
                .unwrap_or(parent_state.children.len());
            parent_state.children.insert(index, child);
        }
    }

    fn clear_ref(state: &NodeState) {
        if let Some(node_ref) = state.node.as_element().and_then(|e| e.node_ref().as_ref()) {
            node_ref.set(None);
        }
    }
}

fn measure(
    paragraph: &Paragraph,
    known: taffy::Size<Option<f32>>,
    available: taffy::Size<AvailableSpace>,
) -> taffy::Size<f32> {
    if let taffy::Size {
        width: Some(width),
        height: Some(height),
    } = known
    {
        return taffy::Size { width, height };
    }
    let max_width = known
        .width
        .map(|width| width as usize)
        .or(match available.width {
            AvailableSpace::Definite(width) => Some(width.max(0.0) as usize),
            AvailableSpace::MinContent => Some(paragraph.min_content_width()),
            AvailableSpace::MaxContent => None,
        });
    let (width, height) = paragraph.measure(max_width);
    taffy::Size {
        width: known.width.unwrap_or(width as f32),
        height: known.height.unwrap_or(height as f32),
    }
}

struct Layout {
    taffy: TaffyTree<Paragraph>,
    keys: HashMap<NodeId, Key>,
    root: NodeId,
}

struct Painter<'a> {
    nodes: &'a HashMap<Key, NodeState>,
    layout: &'a Layout,
    buffer: &'a mut Buffer,
    handlers: &'a mut Handlers,
}

impl Painter<'_> {
    /// Paint a node at its parent's absolute position, clipped to `clip`
    fn paint(
        &mut self,
        key: Key,
        id: NodeId,
        parent_x: i32,
        parent_y: i32,
        clip: Rect,
        inherited: Style,
    ) {
        let (Some(state), Ok(layout)) = (self.nodes.get(&key), self.layout.taffy.layout(id)) else {
            return;
        };
        if state
            .style
            .as_ref()
            .is_some_and(|style| style.layout.display == taffy::Display::None)
        {
            return;
        }
        let x = parent_x + layout.location.x as i32;
        let y = parent_y + layout.location.y as i32;
        let area = to_rect(x, y, layout.size.width as i32, layout.size.height as i32);
        let visible = area.intersection(clip);

        if let Some(element) = state.node.as_element() {
            if let Some(node_ref) = element.node_ref() {
                node_ref.set(Some(area));
            }
            if !element.callbacks().is_empty() {
                self.handlers.register(visible, state.node.clone());
            }
        }

        let content = to_rect(
            x + (layout.border.left + layout.padding.left) as i32,
            y + (layout.border.top + layout.padding.top) as i32,
            (layout.size.width
                - layout.border.left
                - layout.border.right
                - layout.padding.left
                - layout.padding.right) as i32,
            (layout.size.height
                - layout.border.top
                - layout.border.bottom
                - layout.padding.top
                - layout.padding.bottom) as i32,
        );

        match (state.node.as_ref(), &state.style) {
            (TuiNode::Text(_), _) => {
                if let Some(paragraph) = self.layout.taffy.get_node_context(id) {
                    self.paint_paragraph(paragraph, Align::Left, area, visible, inherited);
                }
            }
            (TuiNode::Element(_), Some(style)) => {
                let text_style = inherited.patch(style.text);
                if let Some(background) = style.background {
                    self.buffer.set_style(visible, Style::new().bg(background));
                }
                if let Some(border_type) = style.border {
                    let mut block = Block::bordered()
                        .border_type(border_type)
                        .border_style(text_style.patch(style.border_style));
                    if let Some(title) = &style.title {
                        block = block.title(title.as_str());
                    }
                    render_clipped(block, area, visible, self.buffer);
                }
                let clip = if style.clip {
                    content.intersection(clip)
                } else {
                    clip
                };
                if style.kind == Kind::Text {
                    if let Some(paragraph) = self.layout.taffy.get_node_context(id) {
                        let visible = content.intersection(clip);
                        self.paint_paragraph(paragraph, style.align, content, visible, text_style);
                    }
                } else if let Ok(children) = self.layout.taffy.children(id) {
                    for child in children {
                        if let Some(child_key) = self.layout.keys.get(&child) {
                            self.paint(*child_key, child, x, y, clip, text_style);
                        }
                    }
                }
            }
            (TuiNode::Element(_), None) => {}
        }
    }

    fn paint_paragraph(
        &mut self,
        paragraph: &Paragraph,
        align: Align,
        area: Rect,
        visible: Rect,
        style: Style,
    ) {
        for (row, line) in paragraph.wrap(Some(area.width as usize)).iter().enumerate() {
            let y = area.y as usize + row;
            if y < visible.top() as usize {
                continue;
            }
            if y >= visible.bottom() as usize {
                break;
            }
            let offset = match align {
                Align::Left => 0,
                Align::Center => (area.width as usize).saturating_sub(line.width()) / 2,
                Align::Right => (area.width as usize).saturating_sub(line.width()),
            };
            let mut x = area.x as usize + offset;
            for (symbol, width, grapheme_style) in line.graphemes() {
                if x >= visible.left() as usize && x + width <= visible.right() as usize {
                    self.buffer.set_stringn(
                        x as u16,
                        y as u16,
                        symbol,
                        width,
                        style.patch(grapheme_style),
                    );
                }
                x += width;
            }
        }
    }
}

/// Convert a possibly negative or overflowing area into a ratatui Rect
fn to_rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    let left = x.clamp(0, u16::MAX as i32);
    let top = y.clamp(0, u16::MAX as i32);
    let right = (x + width.max(0)).clamp(left, u16::MAX as i32);
    let bottom = (y + height.max(0)).clamp(top, u16::MAX as i32);
    Rect::new(
        left as u16,
        top as u16,
        (right - left) as u16,
        (bottom - top) as u16,
    )
}

/// Render a widget into `area` but only keep the cells inside `visible`
fn render_clipped(widget: impl Widget, area: Rect, visible: Rect, buffer: &mut Buffer) {
    if visible.is_empty() {
        return;
    }
    if area == visible {
        widget.render(area, buffer);
        return;
    }
    let mut scratch = Buffer::empty(area);
    for position in area.intersection(buffer.area).positions() {
        if let (Some(source), Some(target)) = (buffer.cell(position), scratch.cell_mut(position)) {
            *target = source.clone();
        }
    }
    widget.render(area, &mut scratch);
    for position in visible.positions() {
        if let (Some(source), Some(target)) = (scratch.cell(position), buffer.cell_mut(position)) {
            *target = source.clone();
        }
    }
}

impl<B> ObjectModel for TuiModel<B>
where
    B: Backend + SynchronizedOutput,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    type Node = TuiNode;

    fn create(
        &mut self,
        node: &Arc<TuiNode>,
        parent: &Arc<TuiNode>,
        sibling: &Option<Arc<TuiNode>>,
    ) {
        self.nodes
            .insert(key(node), NodeState::new(node, Some(key(parent))));
        self.insert_child(key(parent), key(node), sibling.as_ref().map(key));
    }

    fn move_node(
        &mut self,
        node: &Arc<TuiNode>,
        parent: &Arc<TuiNode>,
        sibling: &Option<Arc<TuiNode>>,
    ) {
        let (node_key, parent_key) = (key(node), key(parent));
        let Some(state) = self.nodes.get_mut(&node_key) else {
            return;
        };
        let old_parent = state.parent.replace(parent_key);
        if let Some(old_parent) = old_parent.and_then(|old| self.nodes.get_mut(&old)) {
            old_parent.children.retain(|child| *child != node_key);
        }
        self.insert_child(parent_key, node_key, sibling.as_ref().map(key));
    }

    fn remove(&mut self, node: &Arc<TuiNode>, parent: &Arc<TuiNode>) {
        if let Some(parent_state) = self.nodes.get_mut(&key(parent)) {
            parent_state.children.retain(|child| *child != key(node));
        }
        // Descendants are removed by subsequent calls from bloom-core
        if let Some(state) = self.nodes.remove(&key(node)) {
            Self::clear_ref(&state);
        }
    }

    fn update(&mut self, node: &Arc<TuiNode>, next: &Arc<TuiNode>) {
        let (old_key, new_key) = (key(node), key(next));
        let Some(mut state) = self.nodes.remove(&old_key) else {
            return;
        };
        if let (Some(old), Some(new)) = (state.node.as_element(), next.as_element()) {
            if old.node_ref() != new.node_ref() {
                Self::clear_ref(&state);
            }
        }
        state.node = next.clone();
        state.style = parse_style(next);
        for child in &state.children {
            if let Some(child_state) = self.nodes.get_mut(child) {
                child_state.parent = Some(new_key);
            }
        }
        if let Some(parent_state) = state.parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            for child in parent_state.children.iter_mut() {
                if *child == old_key {
                    *child = new_key;
                }
            }
        }
        if self.root == old_key {
            self.root = new_key;
        }
        self.nodes.insert(new_key, state);
    }

    fn finalize(&mut self) -> impl Future<Output = ()> + Send {
        if let Err(error) = self.draw() {
            let mut shared = self.shared.lock().expect("TUI state poisoned");
            shared.error = Some(error);
            // Wake up the render loop's owner so it can shut down
            if let Some(signal) = &shared.signal {
                signal.close();
            }
        }
        future::ready(())
    }

    fn subscribe(&mut self, signal: Sender<()>) {
        self.shared.lock().expect("TUI state poisoned").signal = Some(signal);
    }

    fn get_context(&mut self) -> Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>> {
        self.context.clone()
    }
}
