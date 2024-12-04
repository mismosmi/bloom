use std::{cell::RefCell, collections::HashSet};

thread_local! {
    static INTERNED_STRINGS: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
}

pub(crate) fn interned(s: String) -> &'static str {
    INTERNED_STRINGS.with(|interned_strings| {
        let mut interned_strings = interned_strings.borrow_mut();
        if let Some(interned) = interned_strings.get(&s[..]) {
            return *interned;
        }
        let interned = Box::leak(s.to_string().into_boxed_str());
        interned_strings.insert(interned);
        interned
    })
}
