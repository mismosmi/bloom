use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque},
    hash::{BuildHasher, DefaultHasher, Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

/// Provides an identity used to detect changes to hook arguments.
///
/// Primitives and common standard types implement this trait. References and
/// smart pointers forward to their contents; tuples and containers combine their
/// contents' identities. Sequence order matters, while map and set order does not.
///
/// For structs and enums, `#[derive(Identify)]` combines each field's identity
/// and, for enums, the variant discriminant. All fields must implement `Identify`.
/// Types can implement both [`Hash`] and `Identify`, with independent semantics.
/// Use [`ByHash`] to opt into an identity based on an arbitrary type's `Hash`.
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
/// to remain stable across Rust versions. Floating-point identities use their
/// bit representations, distinguishing signed zero and NaN payloads.
pub trait Identify {
    fn id(&self) -> u64;
}

/// Opts into a [`Hash`]-based identity for the wrapped value.
///
/// ```
/// use bloom_core::{ByHash, Identify};
///
/// #[derive(Hash)]
/// struct Key(u64);
/// assert_eq!(ByHash(Key(1)).id(), ByHash(Key(1)).id());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByHash<T>(pub T);

impl<T: Hash> Identify for ByHash<T> {
    fn id(&self) -> u64 {
        hash(&self.0)
    }
}

fn hash<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

macro_rules! identify_by_hash {
    ($($ty:ty),* $(,)?) => {
        $(impl Identify for $ty {
            fn id(&self) -> u64 { hash(self) }
        })*
    };
}

identify_by_hash!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    str,
    String,
    std::ffi::CStr,
    std::ffi::CString,
    std::ffi::OsStr,
    std::ffi::OsString,
    std::path::Path,
    std::path::PathBuf,
    std::time::Duration,
    std::time::SystemTime,
    std::time::Instant,
    std::net::IpAddr,
    std::net::Ipv4Addr,
    std::net::Ipv6Addr,
    std::net::SocketAddr,
    std::net::SocketAddrV4,
    std::net::SocketAddrV6,
    std::num::NonZeroU8,
    std::num::NonZeroU16,
    std::num::NonZeroU32,
    std::num::NonZeroU64,
    std::num::NonZeroU128,
    std::num::NonZeroUsize,
    std::num::NonZeroI8,
    std::num::NonZeroI16,
    std::num::NonZeroI32,
    std::num::NonZeroI64,
    std::num::NonZeroI128,
    std::num::NonZeroIsize,
    std::ops::RangeFull,
    std::cmp::Ordering,
    std::convert::Infallible,
);

impl Identify for f32 {
    fn id(&self) -> u64 {
        hash(&self.to_bits())
    }
}

impl Identify for f64 {
    fn id(&self) -> u64 {
        hash(&self.to_bits())
    }
}

macro_rules! forward_identity {
    ($($ty:ty),* $(,)?) => {
        $(impl<T: Identify + ?Sized> Identify for $ty {
            fn id(&self) -> u64 { (**self).id() }
        })*
    };
}

forward_identity!(&T, &mut T, Box<T>, Rc<T>, Arc<T>);

impl<T: Identify + ToOwned + ?Sized> Identify for Cow<'_, T> {
    fn id(&self) -> u64 {
        self.as_ref().id()
    }
}

impl<T: ?Sized> Identify for std::marker::PhantomData<T> {
    fn id(&self) -> u64 {
        hash(&())
    }
}

macro_rules! identify_tuple {
    ($($ty:ident:$index:tt),+) => {
        impl<$($ty: Identify),+> Identify for ($($ty,)+) {
            fn id(&self) -> u64 { hash(&($(self.$index.id(),)+)) }
        }
    };
}

identify_tuple!(A:0);
identify_tuple!(A:0, B:1);
identify_tuple!(A:0, B:1, C:2);
identify_tuple!(A:0, B:1, C:2, D:3);
identify_tuple!(A:0, B:1, C:2, D:3, E:4);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
identify_tuple!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);

impl<T: Identify> Identify for Option<T> {
    fn id(&self) -> u64 {
        hash(&self.as_ref().map(Identify::id))
    }
}

impl<T: Identify, E: Identify> Identify for Result<T, E> {
    fn id(&self) -> u64 {
        hash(&self.as_ref().map(Identify::id).map_err(Identify::id))
    }
}

fn sequence_id(values: impl IntoIterator<Item = u64>) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut len = 0usize;
    for id in values {
        id.hash(&mut hasher);
        len += 1;
    }
    len.hash(&mut hasher);
    hasher.finish()
}

fn unordered_id(values: impl IntoIterator<Item = u64>) -> u64 {
    let mut ids: Vec<_> = values.into_iter().collect();
    ids.sort_unstable();
    sequence_id(ids)
}

impl<T: Identify> Identify for [T] {
    fn id(&self) -> u64 {
        sequence_id(self.iter().map(Identify::id))
    }
}

impl<T: Identify, const N: usize> Identify for [T; N] {
    fn id(&self) -> u64 {
        self.as_slice().id()
    }
}

macro_rules! identify_sequence {
    ($($ty:ident),* $(,)?) => {
        $(impl<T: Identify> Identify for $ty<T> {
            fn id(&self) -> u64 { sequence_id(self.iter().map(Identify::id)) }
        })*
    };
}

identify_sequence!(Vec, VecDeque, LinkedList);

impl<T: Identify> Identify for BinaryHeap<T> {
    fn id(&self) -> u64 {
        unordered_id(self.iter().map(|value| value.id()))
    }
}

impl<T: Identify> Identify for BTreeSet<T> {
    fn id(&self) -> u64 {
        unordered_id(self.iter().map(|value| value.id()))
    }
}

impl<K: Identify, V: Identify> Identify for BTreeMap<K, V> {
    fn id(&self) -> u64 {
        unordered_id(self.iter().map(|value| value.id()))
    }
}

impl<T: Identify, S: BuildHasher> Identify for HashSet<T, S> {
    fn id(&self) -> u64 {
        unordered_id(self.iter().map(|value| value.id()))
    }
}

impl<K: Identify, V: Identify, S: BuildHasher> Identify for HashMap<K, V, S> {
    fn id(&self) -> u64 {
        unordered_id(self.iter().map(|value| value.id()))
    }
}

impl<T: Identify> Identify for std::ops::Range<T> {
    fn id(&self) -> u64 {
        hash(&(self.start.id(), self.end.id()))
    }
}

impl<T: Identify + PartialOrd> Identify for std::ops::RangeInclusive<T> {
    fn id(&self) -> u64 {
        hash(&(self.start().id(), self.end().id(), self.is_empty()))
    }
}

impl<T: Identify> Identify for std::ops::RangeFrom<T> {
    fn id(&self) -> u64 {
        self.start.id()
    }
}

impl<T: Identify> Identify for std::ops::RangeTo<T> {
    fn id(&self) -> u64 {
        self.end.id()
    }
}

impl<T: Identify> Identify for std::ops::RangeToInclusive<T> {
    fn id(&self) -> u64 {
        self.end.id()
    }
}

impl<T: Identify> Identify for std::ops::Bound<T> {
    fn id(&self) -> u64 {
        match self {
            Self::Unbounded => hash(&(0u8,)),
            Self::Included(value) => hash(&(1u8, value.id())),
            Self::Excluded(value) => hash(&(2u8, value.id())),
        }
    }
}

impl<T: Identify> Identify for std::cmp::Reverse<T> {
    fn id(&self) -> u64 {
        self.0.id()
    }
}

impl<T: Identify> Identify for std::num::Wrapping<T> {
    fn id(&self) -> u64 {
        self.0.id()
    }
}

impl<T: Identify> Identify for std::num::Saturating<T> {
    fn id(&self) -> u64 {
        self.0.id()
    }
}
