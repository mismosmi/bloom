use std::{
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    ptr,
    sync::atomic::{AtomicU16, Ordering},
};

#[derive(Debug, Default)]
pub struct DomRef(AtomicU16);

thread_local! {
    static HTML_ELEMENT_MAP: RefCell<HashMap<u16, web_sys::Node>> = RefCell::new(HashMap::new());
}

impl DomRef {
    pub fn set(&self, element: web_sys::Node) {
        HTML_ELEMENT_MAP.with(|map| {
            let mut map = map.borrow_mut();
            let current_key = self.0.load(Ordering::Relaxed);
            let key = if current_key != 0 {
                current_key
            } else {
                let mut key = 1;
                while map.contains_key(&key) {
                    key += 1;
                    if key == u16::MAX {
                        panic!("Element Map Overflow");
                    }
                }
                self.0.store(key, Ordering::Relaxed);
                key
            };
            map.insert(key, element);
        });
    }

    pub fn get(&self) -> Option<web_sys::Node> {
        HTML_ELEMENT_MAP.with(|map| {
            map.borrow()
                .get(&self.0.load(Ordering::Relaxed))
                .map(|element| element.clone())
        })
    }
}

impl Drop for DomRef {
    fn drop(&mut self) {
        HTML_ELEMENT_MAP.with(|map| {
            map.borrow_mut().remove(&self.0.load(Ordering::Relaxed));
        });
    }
}

impl Hash for DomRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.load(Ordering::Relaxed).hash(state);
    }
}

impl PartialEq for DomRef {
    fn eq(&self, other: &Self) -> bool {
        ptr::addr_eq(self.0.as_ptr(), other.0.as_ptr())
    }
}
