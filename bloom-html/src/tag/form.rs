use std::sync::Arc;

use bloom_core::{Element, MaybeSend, MaybeSync};
use js_sys::{
    JsString,
    wasm_bindgen::{JsCast, JsValue},
};
use web_sys::{Event, HtmlFormElement};

use crate::{DomRef, HtmlNode, element::HtmlElementBuilder};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type FormAction = Box<dyn Fn(FormData) + Send + Sync + 'static>;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type FormAction = Box<dyn Fn(FormData) + 'static>;

pub struct Form;

impl Form {
    pub fn builder<E>() -> HtmlFormElementBuilder<E> {
        HtmlFormElementBuilder {
            base: HtmlNode::element("form"),
            action: None,
            children: Vec::new(),
        }
    }
}

pub struct HtmlFormElementBuilder<E> {
    base: HtmlElementBuilder<&'static str>,
    action: Option<FormAction>,
    children: Vec<Element<HtmlNode, E>>,
}

impl<E> HtmlFormElementBuilder<E> {
    pub fn attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        V: Into<String>,
    {
        self.base = self.base.attr(key, value);
        self
    }

    pub fn on<K, C>(mut self, key: K, handler: C) -> Self
    where
        K: Into<String>,
        C: Fn(web_sys::Event) + MaybeSend + MaybeSync + 'static,
    {
        self.base = self.base.on(key, handler);
        self
    }

    pub fn dom_ref(mut self, dom_ref: Arc<DomRef>) -> Self {
        self.base = self.base.dom_ref(dom_ref);
        self
    }

    pub fn action<A>(mut self, action: A) -> Self
    where
        A: Fn(FormData) + MaybeSend + MaybeSync + 'static,
    {
        self.action = Some(Box::new(action));
        self
    }

    pub fn children(mut self, children: Vec<Element<HtmlNode, E>>) -> Self {
        self.children = children.into();
        self
    }

    pub fn build(mut self) -> Element<HtmlNode, E> {
        let with_action = match (self.base.callbacks.remove("submit"), self.action) {
            (Some(submit_handler), Some(action)) => self.base.on("submit", move |event| {
                submit_handler(event.clone());
                if !event.default_prevented() {
                    let form_data = get_form_data(event);
                    action(form_data);
                }
            }),
            (None, Some(action)) => self.base.on("submit", move |event| {
                let form_data = get_form_data(event);
                action(form_data);
            }),
            (Some(submit_handler), None) => self.base.on("submit", submit_handler),
            (None, None) => self.base,
        };

        with_action.build().into()
    }
}

pub struct FormData(web_sys::FormData);

pub enum FormDataEntryValue {
    String(String),
    Blob(web_sys::Blob),
    File(web_sys::File),
}

impl FormDataEntryValue {
    fn from_js(jsv: JsValue) -> Self {
        if jsv.has_type::<web_sys::File>() {
            FormDataEntryValue::File(jsv.unchecked_into::<web_sys::File>())
        } else if jsv.has_type::<web_sys::Blob>() {
            FormDataEntryValue::Blob(jsv.unchecked_into::<web_sys::Blob>())
        } else {
            FormDataEntryValue::String(
                jsv.dyn_into::<JsString>()
                    .expect("FormData.get returned unexpected type")
                    .into(),
            )
        }
    }

    pub fn as_string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_blob(self) -> Option<web_sys::Blob> {
        match self {
            Self::Blob(value) => Some(value),
            Self::File(value) => Some(value.unchecked_into()),
            _ => None,
        }
    }

    pub fn as_file(self) -> Option<web_sys::File> {
        match self {
            Self::File(value) => Some(value),
            _ => None,
        }
    }
}

impl FormData {
    pub fn get(&self, key: &str) -> Option<FormDataEntryValue> {
        let value = self.0.get(key);
        if value.is_null() {
            None
        } else {
            Some(FormDataEntryValue::from_js(value))
        }
    }

    pub fn get_all(&self, key: &str) -> Vec<FormDataEntryValue> {
        self.0
            .get_all(key)
            .into_iter()
            .map(|value| FormDataEntryValue::from_js(value))
            .collect()
    }

    pub fn raw(&self) -> &web_sys::FormData {
        &self.0
    }
}

fn get_form_data(event: Event) -> FormData {
    let event_target = event.target().expect("Event target is undefined");
    let form_element: &HtmlFormElement = event_target
        .dyn_ref()
        .expect("Submit Event Target is not a form");
    FormData(web_sys::FormData::new_with_form(form_element).expect("Invalid Form Submitter"))
}
