use std::hash::{DefaultHasher, Hash, Hasher};

/// Provides an identity used to detect changes to hook arguments.
///
/// Every [`Hash`] type implements this trait automatically. For other structs and
/// enums, `#[derive(Identify)]` combines each field's identity and, for enums, the
/// variant discriminant. All fields must implement `Identify`.
///
/// Do not derive both `Hash` and `Identify`: the blanket implementation for
/// `Hash` types already provides `Identify`.
///
/// ```
/// use bloom_core::{Identify, use_memo};
///
/// #[derive(Identify)]
/// struct Inputs {
///     label: String,
///     count: usize,
/// }
///
/// let value = use_memo(Inputs { label: "hi".into(), count: 2 }, |inputs| {
///     inputs.label.repeat(inputs.count)
/// });
/// assert_eq!(&*value, "hihi");
/// ```
///
/// Identities are hashes: collisions are possible and values are not guaranteed
/// to remain stable across Rust versions.
pub trait Identify {
    fn id(&self) -> u64;
}

impl<T> Identify for T
where
    T: Hash,
{
    fn id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}
