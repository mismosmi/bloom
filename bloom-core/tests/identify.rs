use bloom_core::{ByHash, Identify, use_memo};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque},
    rc::Rc,
    sync::Arc,
};

// The payload does not participate in identity and deliberately does not implement Hash.
struct Revision {
    version: u64,
    payload: String,
}

impl Identify for Revision {
    fn id(&self) -> u64 {
        self.version
    }
}

#[derive(Identify)]
struct Inputs<T> {
    revision: T,
    count: usize,
}

#[derive(Identify)]
struct Tuple<'a, T, const N: usize>(&'a str, T, [u8; N])
where
    T: Identify;

#[derive(Identify)]
struct Unit;

#[derive(Identify)]
enum Never {}

#[derive(Identify)]
enum Choice<T> {
    Empty,
    Tuple(T),
    Named { value: T },
}

fn revision(version: u64, payload: &str) -> Revision {
    Revision {
        version,
        payload: payload.into(),
    }
}

#[test]
fn primitives_keep_their_hash_identity() {
    let value = 42u64;
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    assert_eq!(value.id(), hasher.finish());
}

#[test]
fn derived_identity_uses_each_fields_identity() {
    let first = Inputs {
        revision: revision(1, "first"),
        count: 2,
    };
    let same = Inputs {
        revision: revision(1, "other payload"),
        count: 2,
    };
    let changed_revision = Inputs {
        revision: revision(2, "first"),
        count: 2,
    };
    let changed_count = Inputs {
        revision: revision(1, "first"),
        count: 3,
    };

    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed_revision.id());
    assert_ne!(first.id(), changed_count.id());
    let value = use_memo(first, |inputs| inputs.revision.payload.repeat(inputs.count));
    assert_eq!(&*value, "firstfirst");
}

#[test]
fn tuple_and_unit_structs_support_generics() {
    fn assert_identify<T: Identify>() {}
    assert_identify::<Never>();
    let first = Tuple("label", revision(1, "first"), [1, 2]);
    let same = Tuple("label", revision(1, "second"), [1, 2]);
    let changed = Tuple("other", revision(1, "first"), [1, 2]);
    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed.id());
    assert_eq!(Unit.id(), Unit.id());
}

#[test]
fn enums_include_the_variant_and_field_identities() {
    let first = Choice::Tuple(revision(1, "first"));
    let same = Choice::Tuple(revision(1, "second"));
    let changed = Choice::Tuple(revision(2, "first"));
    let named = Choice::Named {
        value: revision(1, "first"),
    };
    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed.id());
    assert_ne!(first.id(), named.id());
    assert_ne!(first.id(), Choice::<Revision>::Empty.id());
}

#[test]
fn hash_and_identify_can_coexist() {
    #[derive(Hash, Identify)]
    struct Both {
        value: u64,
    }
    let value = Both { value: 7 };
    assert_eq!(value.id(), Both { value: 7 }.id());

    #[derive(Hash)]
    struct Custom(u64);
    impl Identify for Custom {
        fn id(&self) -> u64 {
            self.0
        }
    }
    let value = Custom(7);
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    assert_eq!(value.id(), 7);
    assert_eq!(ByHash(value).id(), hasher.finish());
}

#[test]
fn pointers_and_references_forward_custom_identity() {
    let value = revision(7, "payload");
    assert_eq!(<&Revision as Identify>::id(&&value), 7);
    let boxed = Box::new(revision(7, "box"));
    assert_eq!(<Box<Revision> as Identify>::id(&boxed), 7);
    assert_eq!(Rc::new(revision(7, "rc")).id(), 7);
    assert_eq!(Arc::new(revision(7, "arc")).id(), 7);
    let mut value = revision(7, "mutable");
    assert_eq!(<&mut Revision as Identify>::id(&&mut value), 7);
    let erased: Box<dyn Identify> = Box::new(value);
    assert_eq!(erased.id(), 7);
    assert_eq!(Arc::<str>::from("hello").id(), "hello".id());
    assert_eq!(
        Cow::Borrowed("hello").id(),
        Cow::<'_, str>::Owned("hello".into()).id()
    );
}

#[test]
fn nested_containers_compose_custom_identities() {
    let first = (
        Some(vec![revision(1, "a"), revision(2, "b")]),
        Ok::<_, Revision>(revision(3, "c")),
    );
    let same = (
        Some(vec![revision(1, "x"), revision(2, "y")]),
        Ok::<_, Revision>(revision(3, "z")),
    );
    let changed = (
        Some(vec![revision(2, "a"), revision(1, "b")]),
        Ok::<_, Revision>(revision(3, "c")),
    );
    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed.id());
    assert_ne!(None::<Revision>.id(), Some(revision(0, "")).id());
    assert_ne!(
        Ok::<_, Revision>(revision(1, "")).id(),
        Err::<Revision, _>(revision(1, "")).id()
    );
    assert_ne!(Vec::<Revision>::new().id(), vec![revision(0, "")].id());

    let array = [revision(1, "a"), revision(2, "b")];
    assert_eq!(array.id(), array.as_slice().id());
    assert_eq!(array.id(), vec![revision(1, "x"), revision(2, "y")].id());
    let deque: VecDeque<_> = [revision(1, "x"), revision(2, "y")].into();
    let list: LinkedList<_> = [revision(1, "x"), revision(2, "y")].into_iter().collect();
    assert_eq!(array.id(), deque.id());
    assert_eq!(array.id(), list.id());
}

#[test]
fn unordered_collections_ignore_iteration_order_and_use_custom_identity() {
    #[derive(Hash, PartialEq, Eq, PartialOrd, Ord)]
    struct Item(u64, &'static str);
    impl Identify for Item {
        fn id(&self) -> u64 {
            self.0
        }
    }
    let first: HashMap<_, _> = [
        (Item(1, "a"), revision(2, "x")),
        (Item(3, "b"), revision(4, "y")),
    ]
    .into();
    let same: HashMap<_, _> = [
        (Item(3, "changed"), revision(4, "z")),
        (Item(1, "changed"), revision(2, "z")),
    ]
    .into();
    let changed: HashMap<_, _> = [
        (Item(1, "a"), revision(4, "x")),
        (Item(3, "b"), revision(2, "y")),
    ]
    .into();
    assert_eq!(first.id(), same.id());
    assert_ne!(first.id(), changed.id());
    let ordered: BTreeMap<_, _> = same.into_iter().collect();
    assert_eq!(first.id(), ordered.id());
    let first: HashSet<_> = [Item(1, "a"), Item(2, "b")].into();
    let same: HashSet<_> = [Item(2, "x"), Item(1, "y")].into();
    assert_eq!(first.id(), same.id());
    let ordered: BTreeSet<_> = same.into_iter().collect();
    assert_eq!(first.id(), ordered.id());
    let heap: BinaryHeap<_> = [Item(2, "x"), Item(1, "y")].into();
    assert_eq!(first.id(), heap.id());
}

#[test]
fn float_identity_uses_bits() {
    assert_eq!(1.5f64.id(), 1.5f64.id());
    assert_ne!(0.0f64.id(), (-0.0f64).id());
    let nan = f32::from_bits(0x7fc00001);
    assert_eq!(nan.id(), nan.id());
    assert_ne!(nan.id(), f32::from_bits(0x7fc00002).id());
}

#[test]
fn ranges_and_bounds_compose_and_distinguish_state() {
    use std::ops::Bound;
    assert_eq!(
        (revision(1, "a")..revision(2, "b")).id(),
        (revision(1, "x")..revision(2, "y")).id()
    );
    assert_ne!(
        Bound::Included(revision(1, "a")).id(),
        Bound::Excluded(revision(1, "a")).id()
    );
    assert_ne!(
        Bound::<Revision>::Unbounded.id(),
        Bound::Included(revision(0, "a")).id()
    );
    let mut range = 1..=1;
    let before = range.id();
    assert_eq!(range.next(), Some(1));
    assert_ne!(before, range.id());
}
