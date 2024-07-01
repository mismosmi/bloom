use proc_macro2::TokenStream;
use quote::quote;
use syn::Expr;
use syn_rsx::{parse2, Node};

#[proc_macro]
pub fn rsx(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let tree = parse2(tokens.into()).expect("Failed to parse RSX");

    transform_children(tree).into()
}

fn transform_node(node: Node) -> TokenStream {
    match node {
        Node::Element(element) => {
            let tag = element.name.to_string();

            if tag.chars().nth(0).unwrap().is_uppercase() {
                let attributes = transform_props(element.attributes);
                let children = if element.children.is_empty() {
                    TokenStream::new()
                } else {
                    let children = transform_children(element.children);
                    quote! {
                        .children(#children)
                    }
                };
                quote! {
                    <#tag>::new()#children #attributes.build().into()
                }
            } else {
                let attributes = transform_attributes(element.attributes);
                let children = if element.children.is_empty() {
                    quote! {
                        .into()
                    }
                } else {
                    let children = transform_children(element.children);
                    quote! {
                        .children(#children)
                    }
                };
                quote! {
                    tag(#tag)#attributes.build()#children
                }
            }
        }
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
        dbg!(actual.to_string());
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
}
