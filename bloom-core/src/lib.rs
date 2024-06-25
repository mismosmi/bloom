mod component;
mod element;
mod hook;
mod render_loop;
mod render_queue;
mod render_stream;
mod state;
mod suspense;

pub use component::Component;
pub use element::Element;
pub use render_loop::{render_loop, ObjectModel};
pub use render_stream::{render_stream, NodeStream};
pub use state::use_state;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
