use bloom_core::{use_memo, Identify};
use std::hash::{DefaultHasher, Hash, Hasher};

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
fn hash_types_keep_their_existing_identity() {
    let value = ("hello", 42);
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
