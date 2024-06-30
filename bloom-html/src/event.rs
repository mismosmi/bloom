pub type HtmlEvent = web_sys::Event;

pub type EventHandler = Box<dyn Fn(HtmlEvent) + Send + Sync + 'static>;

#[cfg(test)]
mod tests {

    use crate::{tag::button, HtmlElement};

    #[test]
    fn build_button() {
        let button: HtmlElement = button().on("click", |_| {}).build().unwrap();

        let cb = button.callbacks().get("click").unwrap();

        let addr: usize = cb.as_ref() as *const _ as *const () as usize;

        println!("{}", addr);
    }
}
