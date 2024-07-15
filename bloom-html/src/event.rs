/// Event handlers are basically just closures that take a web_sys::Event as an argument.
/// This type only provides a convenience for implementing the actual render-functions.
pub type EventHandler = Box<dyn Fn(web_sys::Event) + Send + Sync + 'static>;

#[cfg(test)]
mod tests {

    use crate::{tag::button, HtmlElement};

    #[test]
    fn build_button() {
        let button: HtmlElement = button().on("click", |_| {}).build();

        let _cb = button.callbacks().get("click");
    }
}
