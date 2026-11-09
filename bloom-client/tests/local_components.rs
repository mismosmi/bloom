#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use bloom_core::{
    Element, Identify, Provider, Spawner, component, render_stream, use_context, use_effect,
    use_memo, use_ref, use_state,
};
use bloom_html::{HtmlNode, prelude::*};
use futures_util::{
    StreamExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use std::{cell::Cell, rc::Rc, sync::Arc};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone)]
struct BrowserSpawner {
    _local: Rc<()>,
}
impl LocalSpawn for BrowserSpawner {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        wasm_bindgen_futures::spawn_local(future);
        Ok(())
    }
}

struct TextUrl(String);
impl TextUrl {
    fn new() -> Self {
        let parts = web_sys::js_sys::Array::of1(&"bloom".into());
        let blob = web_sys::Blob::new_with_str_sequence(&parts).unwrap();
        Self(web_sys::Url::create_object_url_with_blob(&blob).unwrap())
    }
}
impl Drop for TextUrl {
    fn drop(&mut self) {
        web_sys::Url::revoke_object_url(&self.0).unwrap();
    }
}

struct LocalArg(Rc<Cell<usize>>);
impl Identify for LocalArg {
    fn id(&self) -> u64 {
        self.0.get() as u64
    }
}

#[component]
async fn FetchText(
    value: Rc<Cell<usize>>,
) -> Result<Element<HtmlNode, reqwest::Error>, reqwest::Error> {
    let context = use_context::<Rc<Cell<usize>>>();
    assert!(Rc::ptr_eq(value, &context));
    let state = use_state(|| Arc::new(value.clone()));
    let reference = use_ref::<Cell<usize>>();
    let memo = use_memo((), |_| Arc::new(Rc::new(Cell::new(1))));
    // Keeping these local values over the await makes this future non-Send.
    let url = TextUrl::new();
    let text = reqwest::get(&url.0).await?.text().await?;
    reference.set(memo.get());
    use_effect(LocalArg(value.clone()), |arg| {
        arg.0.get();
    });
    let value = value.clone();
    Ok(tag("span")
        .on("click", move |_| {
            value.set(value.get() + 1);
            state.set(Arc::new(value.clone()));
        })
        .build()
        .children(vec![text.into()]))
}

#[wasm_bindgen_test(async)]
async fn reqwest_and_local_values_work_in_components() {
    let value = Rc::new(Cell::new(42));
    let element = Provider::builder()
        .value(value.clone())
        .children(vec![FetchText::builder().value(value).build().into()])
        .build()
        .into();
    let mut stream = render_stream(
        element,
        BrowserSpawner {
            _local: Rc::new(()),
        },
    );
    assert!(matches!(
        stream.next().await,
        Some(Ok((HtmlNode::Element(_), _)))
    ));
}

#[wasm_bindgen_test(async)]
async fn spawner_accepts_local_futures_and_results() {
    let value = Rc::new(Cell::new(0));
    let spawner = Spawner::new(BrowserSpawner {
        _local: Rc::new(()),
    });
    let result = spawner
        .spawn_with_handle(async move {
            let url = TextUrl::new();
            let text = reqwest::get(&url.0).await.unwrap().text().await.unwrap();
            value.set(text.len());
            value
        })
        .unwrap()
        .await;
    assert_eq!(result.get(), 5);
}

// Closing the render signal after the fetched text is committed lets the interactive
// loop finish without timing assumptions or an external HTTP service.
#[derive(Default)]
struct RecordingModel {
    signal: Option<async_channel::Sender<()>>,
    text: Rc<std::cell::RefCell<String>>,
}
impl bloom_core::ObjectModel for RecordingModel {
    type Node = HtmlNode;
    fn subscribe(&mut self, signal: async_channel::Sender<()>) {
        self.signal = Some(signal);
    }
    fn create(&mut self, node: &Arc<HtmlNode>, _: &Arc<HtmlNode>, _: &Option<Arc<HtmlNode>>) {
        if let HtmlNode::Text(text) = node.as_ref() {
            self.text.borrow_mut().push_str(text);
        }
    }
    fn remove(&mut self, _: &Arc<HtmlNode>, _: &Arc<HtmlNode>) {}
    fn move_node(&mut self, _: &Arc<HtmlNode>, _: &Arc<HtmlNode>, _: &Option<Arc<HtmlNode>>) {}
    fn update(&mut self, _: &Arc<HtmlNode>, _: &Arc<HtmlNode>) {}
    async fn finalize(&mut self) {
        // This method's future also holds a non-Send reference across an await.
        futures_util::future::ready(()).await;
        if self.text.borrow().contains("bloom") {
            self.signal.as_ref().unwrap().close();
        }
    }
}

#[wasm_bindgen_test(async)]
async fn suspended_reqwest_component_finishes_in_render_loop() {
    let value = Rc::new(Cell::new(42));
    let element = Provider::builder()
        .value(value.clone())
        .children(vec![
            bloom_core::Suspense::builder()
                .fallback("loading")
                .children(vec![FetchText::builder().value(value).build().into()])
                .build()
                .into(),
        ])
        .build()
        .into();
    let model = RecordingModel::default();
    let text = model.text.clone();
    bloom_core::render_loop(
        Arc::new(tag("main").build().into()),
        element,
        BrowserSpawner {
            _local: Rc::new(()),
        },
        model,
    )
    .await
    .unwrap();
    assert!(text.borrow().contains("bloom"));
}
