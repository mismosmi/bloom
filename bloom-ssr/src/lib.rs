//! Server-side rendering and HTML integration examples for Bloom.
//!
//! ## Components
//!
//! Turns a function into a component.
//! The function arguments become the props of the component.
//! Props of type `Option<T>` are optional, all other props are required.
//! The generated builder (from [bon](https://docs.rs/bon)) accepts anything that converts into the prop type.
//! ```
//! use bloom_core::{component, Element};
//! use bloom_html::{prelude::*, HtmlNode};
//! use bloom_rsx::rsx;
//!
//! #[component]
//! pub fn Greeting(name: String, times: Option<usize>) -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(<div>{name.repeat(times.unwrap_or(1))}</div>))
//! }
//!
//! #[component]
//! fn Labelled<T: ToString + PartialEq + Send + Sync + 'static>(
//!     label: T,
//! ) -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(<span>{label.to_string()}</span>))
//! }
//!
//! #[component]
//! fn App() -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(
//!         <Greeting times=2usize name="Hi" />
//!         <Greeting name="Hello" />
//!         <Labelled<i32> label=42 />
//!     ))
//! }
//!
//! # let html = futures_executor::block_on(bloom_ssr::render_to_string(
//! #     App::builder().build().into(),
//! #     futures_executor::ThreadPool::new().unwrap(),
//! # ));
//! # assert_eq!(html, Ok("<div>HiHi</div><div>Hello</div><span>42</span>".to_string()));
//! ```
//!
//! Leaving out a required prop is a compile-time error:
//! ```compile_fail
//! # use bloom_core::{component, Element};
//! # use bloom_html::{prelude::*, HtmlNode};
//! # use bloom_rsx::rsx;
//! #[component]
//! fn Greeting(name: String) -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(<div>{name.clone()}</div>))
//! }
//!
//! let greeting = Greeting::builder().build();
//! ```
//!
//! ## Context providers
//!
//! Provides a value to all components below it, which can read it with [use_context].
//! ```
//! use bloom_core::{component, use_context, Element, Provider};
//! use bloom_html::{prelude::*, HtmlNode};
//! use bloom_rsx::rsx;
//!
//! #[derive(Default)]
//! struct Theme(&'static str);
//!
//! #[component]
//! fn Themed() -> Result<Element<HtmlNode, ()>, ()> {
//!     let theme = use_context::<Theme>();
//!     Ok(rsx!(<div>{theme.0}</div>))
//! }
//!
//! let app: Element<HtmlNode, ()> = rsx!(<Provider value={Theme("dark")}><Themed /></Provider>);
//!
//! // Without rsx
//! let app: Element<HtmlNode, ()> = Provider::builder()
//!     .value(Theme("dark"))
//!     .children(vec![rsx!(<Themed />)])
//!     .build()
//!     .into();
//! # let html = futures_executor::block_on(bloom_ssr::render_to_string(
//! #     app,
//! #     futures_executor::ThreadPool::new().unwrap(),
//! # ));
//! # assert_eq!(html, Ok("<div>dark</div>".to_string()));
//! ```
//!
//! A provider needs a value:
//! ```compile_fail
//! use bloom_core::{Element, Provider};
//! use bloom_html::HtmlNode;
//!
//! let provider: Element<HtmlNode, ()> = Provider::builder().build().into();
//! ```
//!
//! ## Keyed lists
//!
//! A keyed element keeps its state and native nodes when it is moved within a list,
//! and is rebuilt from scratch when its key changes.
//! rsx calls this function for every tag or component with a `key`, so it has to be in scope.
//! `bloom_html::prelude` re-exports it next to `tag`:
//! ```
//! use bloom_core::{component, Element, Fragment};
//! use bloom_html::{prelude::*, HtmlNode};
//! use bloom_rsx::rsx;
//!
//! #[component]
//! fn Row(label: String) -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(<li>{label.clone()}</li>))
//! }
//!
//! let items = vec![(1, "one"), (2, "two")];
//! let lists: Element<HtmlNode, ()> = rsx!(
//!     <ul>{items.iter().map(|(id, label)| rsx!(<li key={id}>{*label}</li>)).collect::<Vec<_>>()}</ul>
//!     <ul>{items.iter().map(|(id, label)| rsx!(<Row key={id} label=*label />)).collect::<Vec<_>>()}</ul>
//!     <dl>{items.iter().map(|(id, label)| rsx!(
//!         <Fragment key={id}><dt>{id.to_string()}</dt><dd>{*label}</dd></Fragment>
//!     )).collect::<Vec<_>>()}</dl>
//! );
//! # let html = futures_executor::block_on(bloom_ssr::render_to_string(
//! #     lists,
//! #     futures_executor::ThreadPool::new().unwrap(),
//! # ));
//! # assert_eq!(html, Ok(concat!(
//! #     "<ul><li>one</li><li>two</li></ul>",
//! #     "<ul><li>one</li><li>two</li></ul>",
//! #     "<dl><dt>1</dt><dd>one</dd><dt>2</dt><dd>two</dd></dl>",
//! # ).to_string()));
//!
//! // Without rsx
//! let item: Element<HtmlNode, ()> = keyed(1, tag("li").build().children(vec!["one".into()]));
//! ```
//!
//! ## Suspense
//!
//! ```
//! use bloom_core::{component, Element, Suspense};
//! use bloom_html::{prelude::*, HtmlNode};
//! use bloom_rsx::rsx;
//!
//! #[component]
//! fn Profile() -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(<div>"Profile"</div>))
//! }
//!
//! #[component]
//! fn App() -> Result<Element<HtmlNode, ()>, ()> {
//!     Ok(rsx!(
//!         <Suspense fallback="Loading...">
//!             <Profile />
//!         </Suspense>
//!     ))
//! }
//! # let html = futures_executor::block_on(bloom_ssr::render_to_string(
//! #     App::builder().build().into(),
//! #     futures_executor::ThreadPool::new().unwrap(),
//! # ));
//! # assert_eq!(html, Ok("<div>Profile</div>".to_string()));
//! ```

mod serializer;
mod spawner;
mod stream;
mod string;

pub use stream::{render_to_stream, StringStream};
pub use string::render_to_string;
