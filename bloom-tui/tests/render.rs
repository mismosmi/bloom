use std::sync::Arc;

use async_trait::async_trait;
use bloom_core::{component, use_state, Component, Element, Suspense};
use bloom_rsx::rsx;
use bloom_tui::{
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    prelude::*,
    ratatui::{
        buffer::Buffer,
        style::{Color, Modifier},
    },
    TestTerminal, TuiNode,
};
use futures_executor::{block_on, ThreadPool};

fn render(element: Element<TuiNode, ()>, width: u16, height: u16) -> TestTerminal {
    TestTerminal::new(element, width, height, ThreadPool::new().unwrap())
}

fn frame(terminal: &TestTerminal) -> Buffer {
    block_on(terminal.next_frame()).expect("render loop stopped")
}

fn symbols(buffer: &Buffer) -> Vec<String> {
    let area = buffer.area;
    (area.top()..area.bottom())
        .map(|y| {
            (area.left()..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn click(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn renders_rows_and_columns() {
    let terminal = render(
        rsx!(
            <column gap=1>
                <row gap=2>
                    "a"
                    "b"
                </row>
                <text>"c"</text>
            </column>
        ),
        6,
        3,
    );
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["a  b  ", "      ", "c     "]
    );
}

#[test]
fn renders_borders_titles_and_padding() {
    let terminal = render(
        rsx!(
            <box border="rounded" title="T" padding="0 1" width=9>
                <text>"hi"</text>
            </box>
        ),
        10,
        3,
    );
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["╭T──────╮ ", "│ hi    │ ", "╰───────╯ "]
    );
}

#[test]
fn wraps_and_aligns_text() {
    let terminal = render(
        rsx!(
            <text width=7 text_align="center">"hello big world"</text>
        ),
        8,
        3,
    );
    assert_eq!(
        symbols(&frame(&terminal)),
        vec![" hello  ", "  big   ", " world  "]
    );
}

#[test]
fn applies_inherited_styles_to_nested_spans() {
    let terminal = render(
        rsx!(
            <box color="red">
                <text>"a" <span bold>"b"</span></text>
            </box>
        ),
        2,
        1,
    );
    let buffer = frame(&terminal);
    assert_eq!(buffer[(0, 0)].fg, Color::Red);
    assert!(!buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(1, 0)].fg, Color::Red);
    assert!(buffer[(1, 0)].modifier.contains(Modifier::BOLD));
}

#[test]
fn renders_wide_characters() {
    let terminal = render(rsx!(<text>"日本"</text>), 5, 1);
    let buffer = frame(&terminal);
    assert_eq!(buffer[(0, 0)].symbol(), "日");
    assert_eq!(buffer[(2, 0)].symbol(), "本");
}

#[test]
fn clips_overflow() {
    let terminal = render(
        rsx!(
            <box width=3 height=1 overflow="hidden">
                <text wrap="none">"abcdef"</text>
            </box>
        ),
        6,
        1,
    );
    assert_eq!(symbols(&frame(&terminal)), vec!["abc   "]);
}

#[component]
fn Counter() -> Result<Element<TuiNode, ()>, ()> {
    let count = use_state(|| 0);
    let increment = count.clone();
    let reset = count.clone();

    Ok(rsx!(
        <column on_key={move |event| {
            if let Event::Key(key) = event {
                if key.code == KeyCode::Char('r') {
                    reset.update(|_| 0);
                }
            }
        }}>
            <text>"Count: " {count.to_string()}</text>
            <box width=3 on_click={move |_| increment.update(|count| *count + 1)}>
                <text>"[+]"</text>
            </box>
        </column>
    ))
}

#[test]
fn handles_clicks_and_keys() {
    let terminal = render(rsx!(<Counter />), 9, 2);
    assert_eq!(symbols(&frame(&terminal)), vec!["Count: 0 ", "[+]      "]);

    // Clicking outside of the button does nothing
    terminal.send(click(5, 1));
    terminal.send(click(1, 1));
    assert_eq!(symbols(&frame(&terminal))[0], "Count: 1 ");

    terminal.send(click(0, 1));
    assert_eq!(symbols(&frame(&terminal))[0], "Count: 2 ");

    terminal.send(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::NONE,
    )));
    assert_eq!(symbols(&frame(&terminal))[0], "Count: 0 ");
}

#[component]
fn List(items: Vec<&'static str>) -> Result<Element<TuiNode, ()>, ()> {
    Ok(tag("column").build().children(
        items
            .iter()
            .map(|item| rsx!(<text>{*item}</text>))
            .collect(),
    ))
}

#[component]
fn Toggle() -> Result<Element<TuiNode, ()>, ()> {
    let expanded = use_state(|| false);
    let toggle = expanded.clone();
    let items = if *expanded {
        vec!["one", "two", "three"]
    } else {
        vec!["one"]
    };

    Ok(rsx!(
        <column>
            <box on_click={move |_| toggle.update(|expanded| !*expanded)}>"toggle"</box>
            <List items=items />
            "end"
        </column>
    ))
}

#[test]
fn updates_and_removes_children() {
    let terminal = render(rsx!(<Toggle />), 6, 5);
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["toggle", "one   ", "end   ", "      ", "      "]
    );

    terminal.send(click(0, 0));
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["toggle", "one   ", "two   ", "three ", "end   "]
    );

    terminal.send(click(0, 0));
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["toggle", "one   ", "end   ", "      ", "      "]
    );
}

#[derive(PartialEq)]
struct Slow;

#[async_trait]
impl Component for Slow {
    type Node = TuiNode;
    type Error = ();

    async fn render(self: Arc<Self>) -> Result<Element<TuiNode, ()>, ()> {
        futures_util::future::ready(()).await;
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(rsx!(<text>"loaded"</text>))
    }
}

#[test]
fn renders_async_components() {
    let terminal = render(Element::Component(Arc::new(Slow)), 6, 1);
    let mut buffer = frame(&terminal);
    while symbols(&buffer)[0] != "loaded" {
        buffer = frame(&terminal);
    }
}

#[component]
fn Row(label: &'static str) -> Result<Element<TuiNode, ()>, ()> {
    let clicks = use_state(|| 0);
    let increment = clicks.clone();

    Ok(rsx!(
        <text on_click={move |_| increment.update(|clicks| *clicks + 1)}>
            {format!("{label}:{}", *clicks)}
        </text>
    ))
}

#[component]
fn ReorderableList() -> Result<Element<TuiNode, ()>, ()> {
    let reversed = use_state(|| false);
    let toggle = reversed.clone();
    let mut labels = vec!["a", "b", "c"];
    if *reversed {
        labels.reverse();
    }

    Ok(rsx!(
        <column>
            <box on_click={move |_| toggle.update(|reversed| !*reversed)}>"reverse"</box>
            {Element::fragment(
                labels
                    .into_iter()
                    .map(|label| rsx!(<Row key={label} label=label />))
                    .collect(),
            )}
        </column>
    ))
}

#[test]
fn keyed_rows_keep_their_state_when_reordered() {
    let terminal = render(rsx!(<ReorderableList />), 7, 4);
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["reverse", "a:0    ", "b:0    ", "c:0    "]
    );

    // Click "a" twice
    terminal.send(click(0, 1));
    frame(&terminal);
    terminal.send(click(0, 1));
    assert_eq!(symbols(&frame(&terminal))[1], "a:2    ");

    // Reversing moves the rows, the state stays with "a"
    terminal.send(click(0, 0));
    assert_eq!(
        symbols(&frame(&terminal)),
        vec!["reverse", "c:0    ", "b:0    ", "a:2    "]
    );

    // Clicks still reach the moved rows
    terminal.send(click(0, 1));
    assert_eq!(symbols(&frame(&terminal))[1], "c:1    ");
}

/// Renders once its gate is opened
struct Gated(async_channel::Receiver<()>);

impl PartialEq for Gated {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[async_trait]
impl Component for Gated {
    type Node = TuiNode;
    type Error = ();

    async fn render(self: Arc<Self>) -> Result<Element<TuiNode, ()>, ()> {
        let _ = self.0.recv().await;
        Ok(rsx!(<text>"ready"</text>))
    }
}

#[test]
fn suspense_shows_fallback_until_children_are_ready() {
    let (open, gate) = async_channel::bounded(1);
    let terminal = render(
        rsx!(
            <Suspense fallback="loading">
                {Element::Component(Arc::new(Gated(gate)))}
            </Suspense>
        ),
        7,
        1,
    );
    assert_eq!(symbols(&frame(&terminal)), vec!["loading"]);

    open.try_send(()).unwrap();
    let mut buffer = frame(&terminal);
    while symbols(&buffer)[0] != "ready  " {
        buffer = frame(&terminal);
    }
}
