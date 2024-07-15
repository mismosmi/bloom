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
}
