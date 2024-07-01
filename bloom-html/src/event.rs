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
