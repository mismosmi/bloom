//! Terminal UI rendering for [bloom](https://crates.io/crates/bloom-core).
//!
//! bloom-tui renders bloom components to the terminal.
//! Elements are laid out with flexbox (via [taffy](https://crates.io/crates/taffy)),
//! painted into a [ratatui](https://ratatui.rs) cell buffer and only the cells that changed
//! since the last frame are written to the terminal, wrapped in synchronized output so frames never tear.
//!
//! ```no_run
//! use std::sync::Arc;
//! use bloom_core::{component, use_state, Element};
//! use bloom_rsx::rsx;
//! use bloom_tui::{prelude::*, render, TuiNode};
//!
//! #[component]
//! fn Counter() -> Result<Element<TuiNode, ()>, ()> {
//!     let count = use_state(|| 0);
//!     let increment = count.clone();
//!
//!     Ok(rsx!(
//!         <column border="rounded" title="Counter" padding="0 1">
//!             <text>"Count: " <span bold color="green">{count.to_string()}</span></text>
//!             <box on_click={move |_| increment.update(|count| *count + 1)}>
//!                 <text reversed>" Click me "</text>
//!             </box>
//!         </column>
//!     ))
//! }
//!
//! # fn main() {
//! let spawner = futures_executor::ThreadPool::new().unwrap();
//! futures_executor::block_on(render(rsx!(<Counter />), spawner)).unwrap();
//! # }
//! ```
//!
//! # Elements
//! * `<box>` / `<row>` lay out their children in a row, `<column>` / `<col>` in a column.
//!   Any other tag name is treated like `<box>`.
//! * `<text>` / `<span>` / `<p>` are paragraphs: all text inside them, including nested elements,
//!   is flowed and wrapped as one piece of text. Nested elements only contribute their text style.
//! * Bare text inside a box is wrapped like a paragraph without any style of its own.
//!
//! # Attributes
//! Layout (numbers are terminal cells, percentages are relative to the parent):
//! `direction` (`row`, `column`, `row-reverse`, `column-reverse`), `wrap`, `grow`, `shrink`, `basis`,
//! `width`, `height`, `min_width`, `min_height`, `max_width`, `max_height`,
//! `padding`, `padding_x`, `padding_y`, `padding_top`, ..., `margin`, `margin_x`, ..., `gap`, `row_gap`, `column_gap`,
//! `justify_content` / `justify`, `align_items` / `align`, `align_self`,
//! `position` (`relative`, `absolute`), `top`, `right`, `bottom`, `left`,
//! `overflow` (`hidden` clips children), `display` (`none` hides the element).
//! `padding` and `margin` accept CSS-like shorthands: `padding="1 2"`.
//!
//! Appearance: `border` (`plain`, `rounded`, `double`, `thick`), `border_color`, `title`,
//! `color` / `fg`, `bg` / `background`, `bold`, `dim`, `italic`, `underline`, `reversed`, `strikethrough`.
//! Colors are names (`red`, `light-blue`), hex (`#ff8800`) or indices (`42`).
//! Text styles are inherited by everything inside an element.
//!
//! Text: `wrap` (`word`, `char`, `none`) and `text_align` (`left`, `center`, `right`) on paragraphs.
//!
//! # Events
//! Handlers receive the crossterm [Event](crossterm::event::Event):
//! * `on_key`, `on_paste`, `on_resize` and `on_focus` are called on every element that defines them.
//! * `on_click`, `on_scroll` and `on_mouse` are called on the topmost element under the mouse that defines them.
//!
//! Use [use_exit] to quit the application, by default Ctrl+C quits as well.

mod events;
mod model;
mod node;
mod node_ref;
mod style;
mod text;
#[cfg(feature = "tokio")]
mod tokio_spawner;

use std::{
    any::Any,
    fmt::{self, Debug, Display},
    io::{self, stdout},
    pin::pin,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Once,
    },
};

use async_trait::async_trait;
use bloom_core::{render_loop, use_context, Component, Element};
use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::{
    future::{select, Either},
    task::{Spawn, SpawnExt},
    StreamExt,
};
use model::{SharedState, TuiModel};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    Terminal,
};

pub use crossterm;
pub use node::{tag, EventHandler, TuiElement, TuiElementBuilder, TuiNode};
pub use node_ref::NodeRef;
pub use ratatui;
#[cfg(feature = "tokio")]
pub use tokio_spawner::TokioSpawner;

/// shortcut for generating text-nodes
pub fn text<E, T>(text: T) -> Element<TuiNode, E>
where
    T: ToString,
{
    Element::Node(TuiNode::text(text.to_string()), Vec::new())
}

/// Make sure to import `bloom_tui::prelude::*` wherever you want to use
/// [bloom-rsx](https://crates.io/crates/bloom-rsx) to render TuiNodes
pub mod prelude {
    /// The `tag`-function rsx will use to generate TuiElements
    pub use super::tag;
}

/// Errors that end the TUI
#[derive(Debug)]
pub enum Error<E> {
    /// A component returned an error
    Render(E),
    /// Reading from or writing to the terminal failed
    Io(io::Error),
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(error) => write!(f, "Render error: {}", error),
            Self::Io(error) => write!(f, "Terminal error: {}", error),
        }
    }
}

impl<E: Debug + Display> std::error::Error for Error<E> {}

impl<E> From<io::Error> for Error<E> {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A handle to stop rendering, obtain it with [use_exit].
#[derive(Default)]
pub struct Exit(Option<SharedState>);

impl Exit {
    /// Stop the render loop, [render] will return after the current frame.
    pub fn exit(&self) {
        if let Some(signal) = self
            .0
            .as_ref()
            .and_then(|shared| shared.lock().ok()?.signal.clone())
        {
            signal.close();
        }
    }
}

/// Obtain a handle to exit the application:
/// ```
/// use bloom_tui::{crossterm::event::Event, use_exit};
///
/// let exit = use_exit();
/// let on_key = move |_: Event| exit.exit();
/// ```
pub fn use_exit() -> Arc<Exit> {
    use_context::<Exit>()
}

/// Options for [render_with_options]
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Capture mouse events so `on_click`, `on_scroll` and `on_mouse` work. Defaults to `true`.
    /// While the mouse is captured, selecting text needs a modifier key in most terminals (usually Shift).
    pub mouse: bool,
    /// Quit when Ctrl+C is pressed. Defaults to `true`.
    pub exit_on_ctrl_c: bool,
    /// Use the kitty keyboard protocol if the terminal supports it,
    /// this makes key combinations like Ctrl+I and Tab distinguishable. Defaults to `true`.
    pub keyboard_enhancement: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            mouse: true,
            exit_on_ctrl_c: true,
            keyboard_enhancement: true,
        }
    }
}

/// Render a bloom element fullscreen in the terminal until [Exit::exit] is called or Ctrl+C is pressed.
/// The spawner is used to run async components, e.g. a `futures_executor::ThreadPool`
/// or `bloom_tui::TokioSpawner` (with the `tokio` feature).
pub async fn render<E, S>(element: Element<TuiNode, E>, spawner: S) -> Result<(), Error<E>>
where
    E: Send + 'static,
    S: Spawn,
{
    render_with_options(element, spawner, RenderOptions::default()).await
}

/// Like [render], with options to configure the terminal.
pub async fn render_with_options<E, S>(
    element: Element<TuiNode, E>,
    spawner: S,
    options: RenderOptions,
) -> Result<(), Error<E>>
where
    E: Send + 'static,
    S: Spawn,
{
    let _guard = TerminalGuard::enter(&options)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let shared = SharedState::default();
    let root = root_node();
    let model = TuiModel::new(
        terminal,
        &root,
        shared.clone(),
        vec![Arc::new(Exit(Some(shared.clone()))) as Arc<dyn Any + Send + Sync>],
    );

    let rendering = pin!(render_loop(root, root_element(element), spawner, model));
    let events = pin!(handle_events(shared.clone(), options.exit_on_ctrl_c));

    match select(rendering, events).await {
        Either::Left((result, _)) => result.map_err(Error::Render)?,
        Either::Right((result, _)) => result?,
    }

    take_error(&shared)
}

/// bloom-core only creates nodes below a component,
/// so the element is wrapped in one that renders it unchanged.
struct Root<E>(Element<TuiNode, E>);

impl<E> PartialEq for Root<E> {
    fn eq(&self, _other: &Self) -> bool {
        // The root is never re-rendered with new props
        true
    }
}

#[async_trait]
impl<E> Component for Root<E>
where
    E: Send + 'static,
{
    type Node = TuiNode;
    type Error = E;

    async fn render(self: Arc<Self>) -> Result<Element<TuiNode, E>, E> {
        Ok(self.0.clone())
    }
}

fn root_element<E: Send + 'static>(element: Element<TuiNode, E>) -> Element<TuiNode, E> {
    match element {
        Element::Component(_) => element,
        element => Element::Component(Arc::new(Root(element))),
    }
}

fn root_node() -> Arc<TuiNode> {
    Arc::new(TuiElement::builder("column").build().into())
}

fn take_error<E>(shared: &SharedState) -> Result<(), Error<E>> {
    match shared
        .lock()
        .ok()
        .and_then(|mut shared| shared.error.take())
    {
        Some(error) => Err(Error::Io(error)),
        None => Ok(()),
    }
}

fn dispatch(shared: &SharedState, event: &Event) {
    let (targets, signal) = {
        let Ok(shared) = shared.lock() else {
            return;
        };
        (shared.handlers.targets(event), shared.signal.clone())
    };
    events::dispatch(targets, event);
    if let (Event::Resize(_, _), Some(signal)) = (event, signal) {
        // Re-render with the new size, an already queued render is just as good
        let _ = signal.try_send(());
    }
}

async fn handle_events(shared: SharedState, exit_on_ctrl_c: bool) -> io::Result<()> {
    let mut events = EventStream::new();
    while let Some(event) = events.next().await {
        let event = event?;
        if let Event::Key(key) = &event {
            if exit_on_ctrl_c
                && key.kind == KeyEventKind::Press
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('c')
            {
                return Ok(());
            }
        }
        dispatch(&shared, &event);
    }
    Ok(())
}

/// Puts the terminal into raw mode on the alternate screen and restores it when dropped,
/// even if the application panics.
struct TerminalGuard;

const MOUSE: u8 = 1;
const KEYBOARD_ENHANCEMENT: u8 = 2;
/// Whether the terminal is set up, and which optional features are enabled
static ACTIVE: AtomicU8 = AtomicU8::new(0);
const ACTIVE_FLAG: u8 = 4;
static PANIC_HOOK: Once = Once::new();

impl TerminalGuard {
    fn enter(options: &RenderOptions) -> io::Result<Self> {
        PANIC_HOOK.call_once(|| {
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                Self::restore();
                hook(info);
            }));
        });

        let keyboard_enhancement = options.keyboard_enhancement
            && terminal::supports_keyboard_enhancement().unwrap_or(false);

        terminal::enable_raw_mode()?;
        let mut stdout = stdout();
        let mut flags = ACTIVE_FLAG;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        if options.mouse {
            execute!(stdout, EnableMouseCapture)?;
            flags |= MOUSE;
        }
        if keyboard_enhancement {
            execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            flags |= KEYBOARD_ENHANCEMENT;
        }
        ACTIVE.store(flags, Ordering::SeqCst);
        Ok(Self)
    }

    fn restore() {
        let flags = ACTIVE.swap(0, Ordering::SeqCst);
        if flags & ACTIVE_FLAG == 0 {
            return;
        }
        let mut stdout = stdout();
        if flags & KEYBOARD_ENHANCEMENT != 0 {
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
        }
        if flags & MOUSE != 0 {
            let _ = execute!(stdout, DisableMouseCapture);
        }
        let _ = execute!(
            stdout,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        Self::restore();
    }
}

/// Renders a bloom element into an in-memory terminal, for testing components:
/// ```
/// use bloom_rsx::rsx;
/// use bloom_tui::{prelude::*, ratatui::buffer::Buffer, TestTerminal};
///
/// let terminal = TestTerminal::new::<(), _>(
///     rsx!(<box border="plain"><text>"Hi!"</text></box>),
///     7,
///     3,
///     futures_executor::ThreadPool::new().unwrap(),
/// );
/// let frame = futures_executor::block_on(terminal.next_frame()).unwrap();
/// assert_eq!(
///     frame,
///     // Like in CSS, children of a column stretch to its full width by default
///     Buffer::with_lines(["┌─────┐", "│Hi!  │", "└─────┘"]),
/// );
/// ```
pub struct TestTerminal {
    frames: async_channel::Receiver<Buffer>,
    shared: SharedState,
}

impl TestTerminal {
    /// Start rendering the element on the spawner.
    /// If a component returns an error, rendering stops and [TestTerminal::next_frame] returns `None`.
    pub fn new<E, S>(element: Element<TuiNode, E>, width: u16, height: u16, spawner: S) -> Self
    where
        E: Send + 'static,
        S: Spawn + Clone + Send + 'static,
    {
        let (frame_sender, frames) = async_channel::unbounded();
        let shared = SharedState::default();
        let root = root_node();
        let terminal =
            Terminal::new(TestBackend::new(width, height)).expect("Test terminal never fails");
        let model = TuiModel::new(
            terminal,
            &root,
            shared.clone(),
            vec![Arc::new(Exit(Some(shared.clone()))) as Arc<dyn Any + Send + Sync>],
        )
        .on_frame(move |buffer| {
            let _ = frame_sender.try_send(buffer.clone());
        });

        spawner
            .spawn(render_test(
                root,
                root_element(element),
                spawner.clone(),
                model,
            ))
            .expect("Failed to spawn render loop");

        Self { frames, shared }
    }

    /// Wait for the next frame to be rendered.
    /// Frames are queued, so every rendered frame is returned exactly once.
    pub async fn next_frame(&self) -> Option<Buffer> {
        self.frames.recv().await.ok()
    }

    /// Dispatch an event to the rendered elements' event handlers, as if it came from the terminal.
    pub fn send(&self, event: Event) {
        dispatch(&self.shared, &event);
    }
}

async fn render_test<E, S>(
    root: Arc<TuiNode>,
    element: Element<TuiNode, E>,
    spawner: S,
    model: TuiModel<TestBackend>,
) where
    E: Send + 'static,
    S: Spawn,
{
    // Errors end the render loop which closes the frame channel
    let _ = render_loop(root, element, spawner, model).await;
}

impl Drop for TestTerminal {
    fn drop(&mut self) {
        Exit(Some(self.shared.clone())).exit();
    }
}
