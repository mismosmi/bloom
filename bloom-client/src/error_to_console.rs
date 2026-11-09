use wasm_bindgen::JsError;
use web_sys::console::error_1;

pub trait ErrorToConsole {
    fn error_to_console(self) -> ();
}

impl ErrorToConsole for Result<(), JsError> {
    fn error_to_console(self) -> () {
        if let Err(error) = self {
            error_1(&error.into());
        }
    }
}

pub fn error_to_console<F>(cb: F)
where
    F: FnOnce() -> Result<(), JsError>,
{
    let result = cb();
    result.error_to_console();
}

pub async fn async_error_to_console<F>(cb: F)
where
    F: AsyncFnOnce() -> Result<(), JsError>,
{
    let result = cb().await;
    result.error_to_console();
}
