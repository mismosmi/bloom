# bloom-gtk

Native GTK4 rendering for [bloom-core](https://crates.io/crates/bloom-core).
Components render to real GTK widgets, so applications look and behave like any other GTK app.

## Setup
bloom-gtk builds on [gtk4-rs](https://gtk-rs.org/gtk4-rs), which needs the GTK4 development files
(`libgtk-4-dev` on Debian and Ubuntu, `gtk4-devel` on Fedora).

## Example
```rust
use bloom_core::{component, use_state, Element};
use bloom_gtk::{gtk, prelude::*, render, GtkNode};
use bloom_rsx::rsx;

#[component]
fn Counter() -> Result<Element<GtkNode, ()>, ()> {
    let count = use_state(|| 0);
    Ok(rsx!(
        <GtkBox spacing=6 margin=12>
            <Label label={count.to_string()} />
            <Button on_clicked={move |_| count.update(|count| *count + 1)}>"Increment"</Button>
        </GtkBox>
    ))
}

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder().application_id("org.example.Counter").build();
    app.connect_activate(|app| {
        let window = gtk::ApplicationWindow::new(app);
        render(&window, rsx!(<Counter />));
        window.present();
    });
    app.run()
}
```
See [bloom-gtk-example](https://github.com/mismosmi/bloom/tree/main/bloom-gtk-example) for a complete todo app.

## Widgets
Widgets are used like components. The typed widgets in `bloom_gtk::widgets` (all in the prelude)
check their properties and signal handlers at compile time:
```rust
rsx!(
    <Entry
        placeholder_text="Name"
        hexpand
        on_changed={|entry| println!("{}", entry.text())}
    />
)
```
* Properties are the widget's GObject properties in snake case. Every widget also has the common
  widget properties (`hexpand`, `halign`, `margin`, `css_classes`, `tooltip_text`, `sensitive`, ...).
  Properties that are not passed keep GTK's defaults, properties that are removed are reset.
* Signal handlers are props named `on_` plus the signal name, e.g. `on_clicked` or
  `on_notify_active`. They receive the widget that emitted the signal.
* Only containers accept children, strings become labels.
* `ref` takes an `Arc<WidgetRef>` from `use_ref` to access the widget in effects.

Available typed widgets: `Button`, `CheckButton`, `Entry`, `Expander`, `FlowBox`, `Frame`,
`GtkBox`, `Image`, `Label`, `LevelBar`, `ListBox`, `ListBoxRow`, `ProgressBar`, `Revealer`,
`ScrolledWindow`, `Separator`, `Spinner`, `Switch` and `ToggleButton`.

Any other widget can be used through lowercase tags, which take properties as strings that are
converted at runtime: `<calendar show_heading=false on_day_selected={|_| ()} />` creates a `GtkCalendar`.
Widgets from other libraries such as libadwaita can be registered with `register_tag`.

## Threading
GTK widgets can only be used on the main thread, while bloom moves components and nodes between
threads. Nodes are therefore plain descriptions of widgets, and the renderer creates and updates
the widgets on the GTK main context. Signal handlers and props must be `Send + Sync`,
use `WidgetRef` instead of capturing widgets. Async components are run on the GTK main context, too,
state updates from other threads are fine.

## Limitations
* Containers supported as parents are `GtkBox`, `ListBox`, `FlowBox` and widgets with a single
  `child` (windows, buttons, frames, scrolled windows, ...). Widgets that place children in named
  slots (`Paned`, `HeaderBar`, `Grid`, `Stack`, `Notebook`) are not supported yet.
* Signal handlers can't return values, signals that expect one get the default
  (`false`, i.e. "not handled").
* Model based lists (`ListView`, `GridView`) are not supported.
