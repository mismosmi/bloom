use bloom_core::{render_stream, Element};
use bloom_html::HtmlNode;
use futures_util::{task::Spawn, StreamExt};

/// render_to_string takes a bloom-core Element and a spawner and returns a string.
/// Prefer using render_to_stream where possible to get the advantages of streaming rendering.
/// This function is useful for testing and other use-cases where you need the full string at once,
/// e.G. if the necessary headers cannot be sent before the full body is rendered.
pub async fn render_to_string<E, S>(element: Element<HtmlNode, E>, spawner: S) -> Result<String, E>
where
    E: Send + 'static,
    S: Spawn + Send + Clone + 'static,
{
    let mut output = String::new();

    let mut stack = vec![(None, render_stream(element, spawner))];

    while let Some((_, stream)) = stack.last_mut() {
        match stream.next().await {
            Some(Ok((node, children))) => match node {
                HtmlNode::Element(element) => {
                    stack.push((Some(element.tag_name().to_string()), children));
                    output.push_str(&format!("<{}>", element.tag_name()));
                }
                HtmlNode::Text(text) => {
                    output.push_str(&text);
                }
                HtmlNode::Comment(comment) => {
                    output.push_str(&format!("<!--{}-->", comment.text()));
                }
            },
            Some(Err(error)) => return Err(error),
            None => {
                if let Some((Some(tag_name), _)) = stack.pop() {
                    output.push_str(&format!("</{}>", tag_name));
                }
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use crate::spawner::TokioSpawner;

    use super::*;

    #[tokio::test]
    async fn render_simple_string() {
        let element = bloom_html::tag::div()
            .build()
            .children(vec![bloom_html::text("foo")]);

        let output = render_to_string::<(), TokioSpawner>(element, TokioSpawner).await;

        assert_eq!(output, Ok("<div>foo</div>".to_string()));
    }

    #[tokio::test]
    async fn render_error_boundary() {
        #[derive(PartialEq)]
        struct Failing;

        #[bloom_core::__private::async_trait]
        impl bloom_core::Component for Failing {
            type Node = bloom_html::HtmlNode;
            type Error = String;

            async fn render(
                self: std::sync::Arc<Self>,
            ) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
                Err("failed".to_string())
            }
        }

        let div = |children| bloom_html::tag::div().build().children(children);
        let boundary = |children| {
            bloom_core::ErrorBoundary::builder()
                .fallback(|error: bloom_core::CaughtError<String>| bloom_html::text(error.as_str()))
                .children(children)
                .build()
                .into()
        };
        // Nodes rendered before the error are dropped along with the children
        let element = div(vec![
            boundary(vec![div(vec![
                bloom_html::text("foo"),
                div(vec![Failing.into()]),
            ])]),
            boundary(vec![bloom_html::text("bar")]),
        ]);

        let output = render_to_string(element, TokioSpawner).await;

        assert_eq!(output, Ok("<div>failedbar</div>".to_string()));
    }
}
