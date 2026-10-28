//! Renders components into real GTK widgets. These tests need a display,
//! run them with `xvfb-run cargo test` on headless machines.
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use bloom_core::{component, use_effect, use_ref, use_state, Element};
use bloom_gtk::{prelude::*, render, GtkNode, WidgetRef};
use bloom_rsx::rsx;
use gtk::glib;

/// Run the main context until bloom has rendered everything that is ready
fn settle() {
    let context = glib::MainContext::default();
    for _ in 0..10 {
        while context.iteration(false) {}
    }
}

/// Run the main context until condition is true, for work that finishes on other threads
fn settle_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "Timed out");
        settle();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Counts calls from signal handlers. Props compare by identity.
#[derive(Clone, Default)]
struct Calls(Arc<AtomicUsize>);

impl Calls {
    fn add(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl PartialEq for Calls {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

fn mount(element: Element<GtkNode, ()>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    render(&root, element);
    settle();
    root
}

/// A compact description of a widget tree, e.g. `Box[Label(hi), Button[Label(ok)]]`
fn describe(widget: &gtk::Widget) -> String {
    let mut description = widget.type_().name().trim_start_matches("Gtk").to_string();
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        description += &format!("({})", label.label());
    }
    // Only look into containers, other widgets have internal children (like an entry's text)
    let container = widget.is::<gtk::Box>()
        || widget.is::<gtk::ListBox>()
        || widget.is::<gtk::ListBoxRow>()
        || widget.is::<gtk::Button>();
    let mut children = Vec::new();
    let mut child = widget.first_child().filter(|_| container);
    while let Some(current) = child {
        child = current.next_sibling();
        children.push(describe(&current));
    }
    if !children.is_empty() {
        description += &format!("[{}]", children.join(", "));
    }
    description
}

fn children(widget: &impl IsA<gtk::Widget>) -> String {
    let mut children = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        children.push(describe(&current));
    }
    children.join(", ")
}

fn find<W: IsA<gtk::Widget>>(widget: &impl IsA<gtk::Widget>, name: &str) -> W {
    fn search(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if widget.widget_name() == name {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = search(&current, name) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    search(widget.upcast_ref(), name)
        .unwrap_or_else(|| panic!("No widget named {name}"))
        .downcast()
        .expect("Widget has a different type")
}

fn click(root: &gtk::Box, name: &str) {
    find::<gtk::Button>(root, name).emit_clicked();
    settle();
}

#[gtk::test]
fn renders_typed_widgets() {
    let root = mount(rsx!(
        <GtkBox name="box" orientation={gtk::Orientation::Vertical} spacing=8 margin=4>
            <Label label="Hello" xalign=0.0 />
            <Button css_classes={["suggested-action"]}>"Click"</Button>
            <Entry name="entry" placeholder_text="Type here" />
        </GtkBox>
    ));

    assert_eq!(
        children(&root),
        "Box[Label(Hello), Button[Label(Click)], Entry]"
    );
    let container = find::<gtk::Box>(&root, "box");
    assert_eq!(container.orientation(), gtk::Orientation::Vertical);
    assert_eq!(container.spacing(), 8);
    assert_eq!(container.margin_bottom(), 4);
    let button = container.first_child().unwrap().next_sibling().unwrap();
    assert!(button.has_css_class("suggested-action"));
    assert_eq!(
        find::<gtk::Entry>(&root, "entry")
            .placeholder_text()
            .unwrap(),
        "Type here"
    );
}

#[component]
fn Counter() -> Result<Element<GtkNode, ()>, ()> {
    let count = use_state(|| 0);
    let increment = count.clone();
    Ok(rsx!(
        <GtkBox>
            <Label label={format!("Count: {}", *count)} />
            {if *count % 2 == 1 { rsx!(<Label label="odd" />) } else { Element::fragment(vec![]) }}
            <Button name="increment" on_clicked={move |_| increment.update(|count| *count + 1)}>
                "Increment"
            </Button>
        </GtkBox>
    ))
}

#[gtk::test]
fn updates_on_signals() {
    let root = mount(rsx!(<Counter />));
    assert_eq!(
        children(&root),
        "Box[Label(Count: 0), Button[Label(Increment)]]"
    );
    let label = root.first_child().unwrap().first_child().unwrap();

    click(&root, "increment");
    assert_eq!(
        children(&root),
        "Box[Label(Count: 1), Label(odd), Button[Label(Increment)]]",
        "The conditional label should be inserted before the button"
    );
    assert_eq!(
        root.first_child().unwrap().first_child().unwrap(),
        label,
        "The label should be updated, not recreated"
    );

    click(&root, "increment");
    assert_eq!(
        children(&root),
        "Box[Label(Count: 2), Button[Label(Increment)]]"
    );
}

#[component]
fn ReversibleList(list_box: bool) -> Result<Element<GtkNode, ()>, ()> {
    let reversed = use_state(|| false);
    let reverse = reversed.clone();
    let mut ids = vec![1, 2, 3];
    if *reversed {
        ids.reverse();
    }
    let items: Vec<Element<GtkNode, ()>> = ids
        .into_iter()
        .map(|id| rsx!(<Label key={id} label={id.to_string()} />))
        .collect();
    let list: Element<GtkNode, ()> = if *list_box {
        rsx!(<ListBox name="list">{items}</ListBox>)
    } else {
        rsx!(<GtkBox name="list">{items}</GtkBox>)
    };
    Ok(rsx!(
        {list}
        <Button name="reverse" on_clicked={move |_| reverse.update(|reversed| !*reversed)} />
    ))
}

#[gtk::test]
fn moves_keyed_children() {
    for list_box in [false, true] {
        let root = mount(rsx!(<ReversibleList list_box={list_box} />));
        let list = find::<gtk::Widget>(&root, "list");
        let rows = if list_box {
            "ListBox[ListBoxRow[Label(1)], ListBoxRow[Label(2)], ListBoxRow[Label(3)]]"
        } else {
            "Box[Label(1), Label(2), Label(3)]"
        };
        assert_eq!(describe(&list), rows);
        let first = list.first_child().unwrap();

        click(&root, "reverse");
        let rows = if list_box {
            "ListBox[ListBoxRow[Label(3)], ListBoxRow[Label(2)], ListBoxRow[Label(1)]]"
        } else {
            "Box[Label(3), Label(2), Label(1)]"
        };
        assert_eq!(describe(&list), rows);
        assert_eq!(
            list.last_child().unwrap(),
            first,
            "The first item should be moved, not recreated"
        );
    }
}

#[component]
fn Switcher() -> Result<Element<GtkNode, ()>, ()> {
    let framed = use_state(|| true);
    let toggle = framed.clone();
    let content: Element<GtkNode, ()> = rsx!(<Label label="content" />);
    let container: Element<GtkNode, ()> = if *framed {
        rsx!(<Frame label="frame">{content}</Frame>)
    } else {
        rsx!(<ScrolledWindow>{content}</ScrolledWindow>)
    };
    Ok(rsx!(
        {container}
        <Button name="toggle" on_clicked={move |_| toggle.update(|framed| !*framed)} />
    ))
}

#[gtk::test]
fn replaces_widgets_of_another_type() {
    let root = mount(rsx!(<Switcher />));
    let frame = root
        .first_child()
        .unwrap()
        .downcast::<gtk::Frame>()
        .unwrap();
    let content = frame.child().unwrap();
    assert_eq!(
        content.downcast_ref::<gtk::Label>().unwrap().label(),
        "content"
    );

    click(&root, "toggle");
    let scrolled = root
        .first_child()
        .unwrap()
        .downcast::<gtk::ScrolledWindow>()
        .expect("The frame should be replaced by a scrolled window");
    assert_eq!(
        scrolled
            .child()
            .unwrap()
            .downcast::<gtk::Viewport>()
            .unwrap()
            .child()
            .unwrap(),
        content,
        "The child should move to the new widget"
    );
    assert_eq!(root.observe_children().n_items(), 2);
}

#[component]
fn Optional() -> Result<Element<GtkNode, ()>, ()> {
    let set = use_state(|| true);
    let unset = set.clone();
    Ok(if *set {
        rsx!(<Button name="button" tooltip_text="tooltip" label="set" on_clicked={move |_| unset.update(|_| false)} />)
    } else {
        rsx!(<Button name="button" />)
    })
}

#[gtk::test]
fn resets_removed_properties() {
    let root = mount(rsx!(<Optional />));
    let button = find::<gtk::Button>(&root, "button");
    assert_eq!(button.tooltip_text().unwrap(), "tooltip");
    assert_eq!(button.label().unwrap(), "set");

    click(&root, "button");
    assert_eq!(button.tooltip_text(), None);
    assert_eq!(button.label().unwrap_or_default(), "", "GTK shows no label");
}

#[component]
fn Generic() -> Result<Element<GtkNode, ()>, ()> {
    let clicks = use_state(|| 0);
    let click = clicks.clone();
    Ok(rsx!(
        <box spacing=3>
            <label name="label" label={format!("clicks: {}", *clicks)} xalign=0.25 justify="center" />
            <button name="button" on_clicked={move |_| click.update(|clicks| *clicks + 1)} />
        </box>
    ))
}

#[gtk::test]
fn renders_generic_tags() {
    let root = mount(rsx!(<Generic />));
    let label = find::<gtk::Label>(&root, "label");
    assert_eq!(label.label(), "clicks: 0");
    assert_eq!(label.xalign(), 0.25);
    assert_eq!(label.justify(), gtk::Justification::Center);
    assert_eq!(
        root.first_child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap()
            .spacing(),
        3
    );

    click(&root, "button");
    assert_eq!(label.label(), "clicks: 1");
}

#[component]
fn Focused() -> Result<Element<GtkNode, ()>, ()> {
    let entry = use_ref::<WidgetRef>();
    use_effect(entry.clone(), |entry| {
        entry
            .get_as::<gtk::Entry>()
            .expect("The ref should point to the entry")
            .set_text("set from effect");
    });
    Ok(rsx!(<Entry name="entry" ref={entry} />))
}

#[gtk::test]
fn refs_point_to_widgets() {
    let root = mount(rsx!(<Focused />));
    assert_eq!(find::<gtk::Entry>(&root, "entry").text(), "set from effect");
}

#[component]
fn Controlled(changes: Calls) -> Result<Element<GtkNode, ()>, ()> {
    let text = use_state(String::new);
    let set_text = text.clone();
    let changes = changes.clone();
    Ok(rsx!(
        <Entry name="entry" text={text.to_string()} on_changed={move |entry| {
            changes.add();
            let value = entry.text().to_string();
            set_text.update(move |_| value);
        }} />
        <Label name="mirror" label={text.to_string()} />
    ))
}

#[gtk::test]
fn keeps_input_that_matches_state() {
    let changes = Calls::default();
    let root = mount(rsx!(<Controlled changes={changes.clone()} />));
    let entry = find::<gtk::Entry>(&root, "entry");

    entry.set_text("typed");
    entry.set_position(2);
    settle();

    assert_eq!(find::<gtk::Label>(&root, "mirror").label(), "typed");
    assert_eq!(
        changes.count(),
        1,
        "Rendering the text the entry already has should not set it again"
    );
    assert_eq!(entry.position(), 2, "The cursor should stay where it was");
}

#[component]
fn Toggle(toggles: Calls) -> Result<Element<GtkNode, ()>, ()> {
    let toggles = toggles.clone();
    Ok(rsx!(
        <Switch name="switch" on_notify_active={move |_| toggles.add()} />
    ))
}

#[gtk::test]
fn handles_detailed_signals() {
    let toggles = Calls::default();
    let root = mount(rsx!(<Toggle toggles={toggles.clone()} />));
    find::<gtk::Switch>(&root, "switch").set_active(true);
    assert_eq!(toggles.count(), 1);
}

#[component]
async fn Loaded() -> Result<Element<GtkNode, ()>, ()> {
    let (sender, receiver) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        let _ = sender.send("loaded");
    });
    let text = receiver.await.map_err(|_| ())?;
    Ok(rsx!(<Label label={text} />))
}

#[gtk::test]
fn renders_async_components_in_place() {
    let root = mount(rsx!(
        <Label label="before" />
        <Loaded />
        <Label label="after" />
    ));
    settle_until(|| children(&root).contains("loaded"));
    assert_eq!(
        children(&root),
        "Label(before), Label(loaded), Label(after)"
    );
}
