use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DataStruct, DeriveInput, Expr, ExprPath, Fields};
use syn_rsx::{parse2, Node, NodeName};

/// The core rsx macro.
/// Transforms
/// * `<Component prop="value" />` into `Component::new().prop("value").build().into()`
/// * `<tag attribute="value" on_event={handler} />` into `tag("tag").attr("attribute", "value").on("event", handler).build().into()`
/// * `"text"` into `"text".to_string().into()`
#[proc_macro]
pub fn rsx(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let tree = parse2(tokens.into()).expect("Failed to parse RSX");

    transform_children(tree).into()
}

fn transform_node(node: Node) -> TokenStream {
    match node {
        Node::Element(element) => match &element.name {
            NodeName::Block(_) => transform_tag(element.name, element.attributes, element.children),
            NodeName::Path(path) => {
                if let Some(ident) = path.path.get_ident() {
                    if ident
                        .to_string()
                        .chars()
                        .nth(0)
                        .expect("Cannot render empty identifier")
                        .is_lowercase()
                    {
                        transform_tag(element.name, element.attributes, element.children)
                    } else {
                        transform_component(path, element.attributes, element.children)
                    }
                } else {
                    transform_component(path, element.attributes, element.children)
                }
            }
            NodeName::Punctuated(_) => {
                transform_tag(element.name, element.attributes, element.children)
            }
        },
        Node::Attribute(_) => {
            panic!("Invalid attribute")
        }
        Node::Block(block) => {
            let value: &Expr = block.value.as_ref();
            quote! {
                #value.into()
            }
        }
        Node::Comment(_) => TokenStream::new(),
        Node::Doctype(_) => TokenStream::new(),
        Node::Fragment(fragment) => transform_children(fragment.children),
        Node::Text(text) => {
            let _text: &Expr = text.value.as_ref();
            quote! { #_text.to_string().into() }
        }
    }
}

fn transform_attributes(attributes: Vec<Node>) -> TokenStream {
    let mut attrs = TokenStream::new();
    attributes
        .into_iter()
        .map(|attribute| match attribute {
            Node::Attribute(attribute) => {
                let name = attribute.key.to_string();

                if name == "ref" {
                    let _value: Expr = attribute.value.expect("Refs must be Arc<DomRef>").into();
                    quote! {
                        .dom_ref(#_value)
                    }
                } else if name.starts_with("on_") {
                    let _value: Expr = attribute.value.expect("Callbacks must be functions").into();
                    let name = name[3..].to_string();
                    quote! {
                        .on(#name, #_value)
                    }
                } else {
                    if let Some(value) = attribute.value {
                        let _value: Expr = value.into();
                        quote! {
                            .attr(#name, #_value)
                        }
                    } else {
                        quote! {
                            .attr(#name, true)
                        }
                    }
                }
            }
            _ => panic!("not an attribute"),
        })
        .for_each(|attr| attrs.extend(attr));
    attrs
}

fn transform_props(attributes: Vec<Node>) -> TokenStream {
    let mut props = TokenStream::new();
    attributes
        .into_iter()
        .map(|attribute| match attribute {
            Node::Attribute(attribute) => {
                let name = attribute.key.to_string();

                if let Some(value) = attribute.value {
                    let value: Expr = value.into();
                    quote! {
                        .#name(#value)
                    }
                } else {
                    quote! {
                        .#name(true)
                    }
                }
            }
            _ => panic!("not an attribute"),
        })
        .for_each(|attr| props.extend(attr));
    props
}

fn transform_children(nodes: Vec<Node>) -> proc_macro2::TokenStream {
    let nodes = nodes.into_iter().map(transform_node);
    let len = nodes.len();

    quote! {
        {
            let mut children = Vec::with_capacity(#len);
            #(children.push(#nodes);)*
            children.into()
        }
    }
}

fn transform_component(tag: &ExprPath, attributes: Vec<Node>, children: Vec<Node>) -> TokenStream {
    let attributes = transform_props(attributes);
    let children = if children.is_empty() {
        TokenStream::new()
    } else {
        let children = transform_children(children);
        quote! {
            .children(#children)
        }
    };
    quote! {
        <#tag>::new()#children #attributes.build().into()
    }
}

fn transform_tag(tag: NodeName, attributes: Vec<Node>, children: Vec<Node>) -> TokenStream {
    let attributes = transform_attributes(attributes);
    let children = if children.is_empty() {
        quote! {
            .into()
        }
    } else {
        let children = transform_children(children);
        quote! {
            .children(#children)
        }
    };
    let tag = tag.to_string();
    quote! {
        tag(#tag)#attributes.build()#children
    }
}

#[proc_macro_derive(NoopBuilder)]
pub fn derive_noop_builder(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(item);

    if let Data::Struct(DataStruct { fields, .. }) = data {
        assert_eq!(
            fields,
            Fields::Unit,
            "NoopBuilder can only be derived for unit structs"
        );
    } else {
        panic!("NoopBuilder can only be derived for unit structs")
    }

    quote! {
        impl #ident {
            fn new() -> Self {
                Self
            }

            fn build(self) -> Self {
                self
            }
        }
    }
    .into()
}

#[cfg(test)]
mod tests {
    use quote::quote;

    #[test]
    fn transform_text() {
        let actual = super::transform_node(
            syn_rsx::parse2(quote! { "hello world" })
                .unwrap()
                .into_iter()
                .nth(0)
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "\"hello world\" . to_string () . into ()"
        );
    }

    #[test]
    fn pass_ref() {
        let actual = super::transform_node(
            syn_rsx::parse2(quote! { <div ref={my_ref}></div> })
                .unwrap()
                .into_iter()
                .nth(0)
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "tag (\"div\") . dom_ref ({ my_ref }) . build () . into ()"
        );
    }

    #[test]
    fn render_component() {
        let actual = super::transform_node(
            syn_rsx::parse2(quote! {
                <MyComponent number_prop=123 boolean_prop>
                  <div id="child" />
                </MyComponent>
            })
            .unwrap()
            .into_iter()
            .nth(0)
            .unwrap(),
        );
        assert_eq!(actual.to_string(), "< MyComponent > :: new () . children ({ let mut children = Vec :: with_capacity (1usize) ; children . push (tag (\"div\") . attr (\"id\" , \"child\") . build () . into ()) ; children . into () }) . \"number_prop\" (123) . \"boolean_prop\" (true) . build () . into ()")
    }
}
