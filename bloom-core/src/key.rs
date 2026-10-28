use std::hash::{DefaultHasher, Hash, Hasher};

/// Identifies an element among its siblings so it keeps its state and native nodes
/// when the list it is part of is reordered.
/// Any hashable value can be a key, only its hash is stored:
/// ```
/// use bloom_core::Key;
///
/// assert_eq!(Key::new("item-1"), Key::new("item-1"));
/// assert_ne!(Key::new("item-1"), Key::new("item-2"));
/// assert_eq!(Key::new("item-1"), Key::new(String::from("item-1")));
/// ```
/// The type is part of the hash, so `Key::new(1)` (an `i32`) and `Key::new(1u64)`
/// are generally different keys.
/// Two distinct values can in theory share a hash, which is negligibly unlikely
/// for the 64 bit hashes used here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key(u64);

impl Key {
    pub fn new<T: Hash>(value: T) -> Self {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        Self(hasher.finish())
    }
}
