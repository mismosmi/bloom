use std::{ptr, sync::Mutex};

use ratatui::layout::Rect;

/// A reference to the area an element occupied in the last rendered frame.
/// Obtain one via
/// ```
/// use bloom_core::use_ref;
/// use bloom_tui::NodeRef;
///
/// let my_ref = use_ref::<NodeRef>();
/// ```
/// and pass it to an element with `ref={my_ref}`.
#[derive(Debug, Default)]
pub struct NodeRef(Mutex<Option<Rect>>);

impl NodeRef {
    /// The area (border-box, in terminal cells) the element was rendered to
    /// or `None` if it has not been rendered yet.
    pub fn area(&self) -> Option<Rect> {
        *self.0.lock().expect("NodeRef poisoned")
    }

    pub(crate) fn set(&self, area: Option<Rect>) {
        *self.0.lock().expect("NodeRef poisoned") = area;
    }
}

impl PartialEq for NodeRef {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self, other)
    }
}
