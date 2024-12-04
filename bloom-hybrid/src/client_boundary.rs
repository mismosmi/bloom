use std::{
    any::{Any, TypeId},
    clone,
    collections::HashMap,
    sync::Arc,
};

use async_trait::async_trait;
use bloom_core::{Component, Element, _get_context, use_state};
use bloom_html::{
    tag::{div, script},
    HtmlNode,
};

struct ClientBoundary<E>
where
    E: 'static,
{
    children: Vec<Element<HtmlNode, E>>,
    component_id: String,
}

impl<E> PartialEq for ClientBoundary<E> {
    fn eq(&self, other: &Self) -> bool {
        self.children == other.children && self.component_id == other.component_id
    }
}

#[async_trait]
impl<E> Component for ClientBoundary<E> {
    type Node = HtmlNode;
    type Error = E;

    async fn render(self: Arc<Self>) -> Result<Element<Self::Node, Self::Error>, Self::Error> {
        let context = use_state(|| Arc::<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>::default());

        Ok(Element::fragment(vec![
            div()
                .attr("hidden", "hidden")
                .attr("data-bloom-partial", &self.component_id)
                .build()
                .into(),
            Element::fragment(self.children.iter().map(Clone::clone).collect()),
            script()
                .attr("type", "module")
                .build()
                .children(vec![format!(
                    "BLOOM['component_{}']({}, {})",
                    &self.component_id, &self.component_id, context_id,
                )
                .into()]),
        ]))
    }
}
