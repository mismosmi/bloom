use js_sys::Error as JsError;
use std::error::Error as RustError;
use web_sys::console::error_1;

pub trait ErrorToConsole {
    fn error_to_console(self) -> ();
}

fn error_to_js_error<E>(rust_error: &E) -> JsError
where
    E: RustError,
{
    let js_error = JsError::new(&rust_error.to_string());
    js_error.set_name("RustError");

    if let Some(source) = rust_error.source() {
        js_error.set_cause(&error_to_js_error(&source));
    }

    js_error
}

impl<E> ErrorToConsole for Result<(), E>
where
    E: RustError,
{
    fn error_to_console(self) -> () {
        if let Err(rust_error) = self {
            let js_error = error_to_js_error(&rust_error);

            error_1(&js_error);
        }
    }
}

pub fn error_to_console<E, F>(cb: F)
where
    E: RustError,
    F: FnOnce() -> Result<(), E>,
{
    let result = cb();
    result.error_to_console();
}
