use bloom_html::HtmlElement;

pub(crate) fn serialize_node_open(node: &HtmlElement) -> String {
    format!(
        "<{}{}>",
        node.tag_name(),
        node.attributes()
            .iter()
            .map(|(key, value)| format!(" {}=\"{}\"", key, value))
            .collect::<String>()
    )
}
