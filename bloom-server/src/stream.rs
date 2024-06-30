use std::task::Poll;

use bloom_core::{render_stream, Element, NodeStream};
use bloom_html::HtmlNode;
use futures_util::{task::Spawn, Stream, StreamExt};

use crate::serializer::serialize_node_open;

pub struct StringStream<E> {
    stack: Vec<(Option<String>, NodeStream<HtmlNode, E>)>,
}

impl<E> Stream for StringStream<E> {
    type Item = Result<String, E>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if let Some(stream) = self.stack.last_mut().map(|item| &mut item.1) {
            match stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok((node, children)))) => match node {
                    HtmlNode::Element(element) => {
                        self.stack
                            .push((Some(element.tag_name().to_string()), children));
                        return Poll::Ready(Some(Ok(serialize_node_open(&element))));
                    }
                    HtmlNode::Text(text) => {
                        return Poll::Ready(Some(Ok(text)));
                    }
                },
                Poll::Ready(None) => {
                    if let Some((Some(tag_name), _)) = self.stack.pop() {
                        Poll::Ready(Some(Ok(format!("</{}>", tag_name))))
                    } else {
                        Poll::Ready(None)
                    }
                }
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            }
        } else {
            Poll::Ready(None)
        }
    }
}

impl<E> StringStream<E> {
    pub fn new(root: NodeStream<HtmlNode, E>) -> Self {
        Self {
            stack: vec![(None, root)],
        }
    }
}

pub fn render_to_stream<E, S>(element: Element<HtmlNode, E>, spawner: S) -> StringStream<E>
where
    E: Send + 'static,
    S: Spawn + Clone + Send + 'static,
{
    StringStream::new(render_stream(element, spawner))
}

#[cfg(test)]
mod tests {
    use bloom_html::{tag::div, text};

    use crate::spawner::TokioSpawner;

    use super::*;

    #[tokio::test]
    async fn render_simple_stream() {
        let element = div().children(vec![text("foo")]);

        let mut stream = render_to_stream::<(), TokioSpawner>(element, TokioSpawner);

        let mut output = String::new();
        while let Some(Ok(chunk)) = stream.next().await {
            output.push_str(&chunk);
        }

        assert_eq!(output, "<div>foo</div>");
    }

    #[tokio::test]
    async fn render_with_attributes() {
        let element = div().attr("class", "foo").attr("id", "bar").into();

        let mut stream = render_to_stream::<(), TokioSpawner>(element, TokioSpawner);

        let mut output = String::new();
        while let Some(Ok(chunk)) = stream.next().await {
            output.push_str(&chunk);
        }

        assert!(output.contains("class=\"foo\""));
        assert!(output.contains("id=\"bar\""));
    }
}
