use bloom_core::{component, use_state, Element};
use bloom_rsx::rsx;
use bloom_tui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    render, use_exit, TuiNode,
};

#[component]
fn Counter() -> Result<Element<TuiNode, ()>, ()> {
    let count = use_state(|| 0);
    let exit = use_exit();
    let on_key = {
        let count = count.clone();
        move |event| {
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => exit.exit(),
                    KeyCode::Up | KeyCode::Char('+') => count.update(|count| *count + 1),
                    KeyCode::Down | KeyCode::Char('-') => count.update(|count| *count - 1),
                    _ => {}
                }
            }
        }
    };
    let increment = count.clone();
    let decrement = count.clone();

    Ok(rsx!(
        <column on_key={on_key} align="center" justify="center" grow=1>
            <column border="rounded" title=" bloom-tui " border_color="cyan" padding="1 3" gap=1 align="center">
                <text>"Count: " <span bold color="green">{count.to_string()}</span></text>
                <row gap=2>
                    <box on_click={move |_| decrement.update(|count| *count - 1)}>
                        <text reversed>" - "</text>
                    </box>
                    <box on_click={move |_| increment.update(|count| *count + 1)}>
                        <text reversed>" + "</text>
                    </box>
                </row>
                <text dim>"↑/↓ or click to count, q to quit"</text>
            </column>
        </column>
    ))
}

fn main() {
    let spawner = futures_executor::ThreadPool::new().expect("Failed to start thread pool");
    futures_executor::block_on(render(rsx!(<Counter />), spawner)).expect("Failed to render");
}
