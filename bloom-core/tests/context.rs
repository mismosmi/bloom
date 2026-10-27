use std::sync::{Arc, Mutex};

use bloom_core::{component, use_context, Element, Provider};
use bloom_html::{prelude::*, HtmlNode};
use bloom_rsx::rsx;

// Deliberately not Clone: consumers must share the provided value.
#[derive(Default)]
struct Shared(Mutex<Vec<&'static str>>);

static SEEN: Mutex<Vec<Arc<Shared>>> = Mutex::new(Vec::new());

#[component]
fn Consumer(name: &'static str) -> Result<Element<HtmlNode, ()>, ()> {
    let shared = use_context::<Shared>();
    shared.0.lock().unwrap().push(name);
    SEEN.lock().unwrap().push(shared);
    Ok(rsx!(<div />))
}

#[test]
fn consumers_share_the_provided_value() {
    let app: Element<HtmlNode, ()> = Provider::new(Shared::default()).children(vec![rsx!(
        <Consumer name="first" />
        <Consumer name="second" />
    )]);

    futures_executor::block_on(bloom_ssr::render_to_string(
        app,
        futures_executor::ThreadPool::new().unwrap(),
    ))
    .unwrap();

    let seen = SEEN.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(Arc::ptr_eq(&seen[0], &seen[1]));
    let mut names = seen[0].0.lock().unwrap().clone();
    names.sort();
    assert_eq!(names, ["first", "second"]);
}
