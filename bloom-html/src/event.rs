/// An event handler with the target's thread-safety requirements.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type EventHandler = Box<dyn Fn(web_sys::Event) + Send + Sync + 'static>;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type EventHandler = Box<dyn Fn(web_sys::Event) + 'static>;

#[cfg(test)]
mod tests {

    use crate::{HtmlElement, tag::button};

    #[test]
    fn build_button() {
        let button: HtmlElement = button().on("click", |_| {}).build();

        let _cb = button.callbacks().get("click");
    }
}
