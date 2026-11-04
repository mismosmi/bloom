//! A todo list rendered with native GTK4 widgets
use bloom_core::{component, use_state, Element};
use bloom_gtk::{gtk, prelude::*, render, GtkNode};
use bloom_rsx::rsx;

#[derive(Clone, PartialEq)]
struct Todo {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Clone, PartialEq, Default)]
struct Todos {
    items: Vec<Todo>,
    next_id: u64,
}

impl Todos {
    fn add(&self, text: String) -> Self {
        let mut todos = self.clone();
        todos.items.push(Todo {
            id: todos.next_id,
            text,
            done: false,
        });
        todos.next_id += 1;
        todos
    }

    fn toggle(&self, id: u64) -> Self {
        let mut todos = self.clone();
        for todo in &mut todos.items {
            if todo.id == id {
                todo.done = !todo.done;
            }
        }
        todos
    }

    fn remove(&self, id: u64) -> Self {
        let mut todos = self.clone();
        todos.items.retain(|todo| todo.id != id);
        todos
    }

    fn clear_done(&self) -> Self {
        let mut todos = self.clone();
        todos.items.retain(|todo| !todo.done);
        todos
    }
}

#[component]
fn App() -> Result<Element<GtkNode, ()>, ()> {
    let todos = use_state(Todos::default);
    let draft = use_state(String::new);

    let add = {
        let todos = todos.clone();
        let draft = draft.clone();
        move || {
            let text = draft.trim().to_string();
            if !text.is_empty() {
                todos.update(move |todos| todos.add(text));
                draft.update(|_| String::new());
            }
        }
    };
    let add_on_enter = add.clone();
    let set_draft = draft.clone();
    let clear = todos.clone();

    let items: Vec<Element<GtkNode, ()>> = todos
        .items
        .iter()
        .map(|todo| {
            let id = todo.id;
            let toggle = todos.clone();
            let remove = todos.clone();
            rsx!(
                <ListBoxRow key={id} activatable=false>
                    <GtkBox spacing=8 margin=6>
                        <CheckButton
                            active={todo.done}
                            on_toggled={move |_| toggle.update(move |todos| todos.toggle(id))}
                        />
                        <Label
                            label={todo.text.clone()}
                            hexpand
                            xalign=0.0
                            css_classes={if todo.done { vec!["dim-label"] } else { vec![] }}
                        />
                        <Button
                            icon_name="user-trash-symbolic"
                            has_frame=false
                            tooltip_text="Remove"
                            on_clicked={move |_| remove.update(move |todos| todos.remove(id))}
                        />
                    </GtkBox>
                </ListBoxRow>
            )
        })
        .collect();

    let done = todos.items.iter().filter(|todo| todo.done).count();
    let status = format!("{done} of {} done", todos.items.len());

    Ok(rsx!(
        <GtkBox orientation={gtk::Orientation::Vertical} spacing=12 margin=12>
            <GtkBox spacing=6>
                <Entry
                    text={draft.to_string()}
                    placeholder_text="What needs to be done?"
                    hexpand
                    on_changed={move |entry| {
                        let text = entry.text().to_string();
                        set_draft.update(move |_| text);
                    }}
                    on_activate={move |_| add_on_enter()}
                />
                <Button
                    css_classes={["suggested-action"]}
                    sensitive={!draft.trim().is_empty()}
                    on_clicked={move |_| add()}
                >
                    "Add"
                </Button>
            </GtkBox>
            <ScrolledWindow vexpand min_content_height=240 has_frame>
                <ListBox selection_mode={gtk::SelectionMode::None} show_separators>
                    {items}
                </ListBox>
            </ScrolledWindow>
            <GtkBox spacing=6>
                <Label label={status} hexpand xalign=0.0 css_classes={["dim-label"]} />
                <Button
                    sensitive={done > 0}
                    on_clicked={move |_| clear.update(|todos| todos.clear_done())}
                >
                    "Clear completed"
                </Button>
            </GtkBox>
        </GtkBox>
    ))
}

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("de.michelsmola.bloom.TodoExample")
        .build();

    app.connect_activate(|app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("bloom todos")
            .default_width(420)
            .default_height(480)
            .build();
        render(&window, rsx!(<App />));
        window.present();
    });

    app.run()
}
