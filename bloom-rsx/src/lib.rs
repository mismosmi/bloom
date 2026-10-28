use proc_macro2::TokenStream;
use quote::quote;
use rstml::node::{atoms::TagGenerics, Node, NodeAttribute, NodeBlock, NodeName};
use rstml::parse2;
use syn::{
    parse_macro_input, Data, DataStruct, DeriveInput, Expr, ExprBlock, ExprPath, Fields, Stmt,
};

/// The core rsx macro.
/// Transforms
/// * `<Component prop="value" />` into `Component::builder().prop("value").build().into()`
/// * `<tag attribute="value" on_event={handler} />` into `tag("tag").attr("attribute", "value").on("event", handler).build().into()`
/// * `"text"` into `"text".to_string().into()`
///
/// A `key` on a tag or component wraps it in a keyed element, which identifies it among its
/// siblings so it keeps its state when its list is reordered:
/// `<li key={id} />` becomes `keyed(id, tag("li").build())`.
/// Like `tag`, a `keyed` function must be in scope (`bloom_html::prelude` provides one).
#[proc_macro]
pub fn rsx(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut tree = parse2(tokens.into()).expect("Failed to parse RSX");

    // A single element is returned as is instead of in a fragment, so a key on it
    // identifies it in the list it is rendered into
    if let [Node::Element(_)] = tree.as_slice() {
        return transform_node(tree.remove(0)).into();
    }

    transform_children(tree).into()
}

fn transform_node(node: Node) -> TokenStream {
    match node {
        Node::Element(element) => {
            let name = element.open_tag.name;
            let generics = element.open_tag.generics;
            let attributes = element.open_tag.attributes;
            let children = element.children;
            match &name {
                NodeName::Path(path) => {
                    let is_tag = path.path.get_ident().is_some_and(|ident| {
                        ident
                            .to_string()
                            .chars()
                            .next()
                            .expect("Cannot render empty identifier")
                            .is_lowercase()
                    });
                    if is_tag {
                        transform_tag(&name, attributes, children)
                    } else {
                        transform_component(path, &generics, attributes, children)
                    }
                }
                NodeName::Block(_) | NodeName::Punctuated(_) => {
                    transform_tag(&name, attributes, children)
                }
            }
        }
        Node::Block(block) => match block {
            NodeBlock::ValidBlock(block) => quote! {
                #block.into()
            },
            NodeBlock::Invalid(_) => panic!("Invalid block"),
        },
        Node::Comment(_) => TokenStream::new(),
        Node::Doctype(_) => TokenStream::new(),
        Node::Fragment(fragment) => transform_children(fragment.children),
        Node::Text(text) => {
            let _text = text.value;
            quote! { #_text.to_string().into() }
        }
        Node::RawText(text) => {
            let _text = text.to_string_best();
            quote! { #_text.to_string().into() }
        }
        Node::Custom(_) => unreachable!("custom nodes are not enabled"),
    }
}

/// `attr={value}` is parsed as a block, unwrap it so the generated code has no unnecessary braces.
fn attribute_value(value: &Expr) -> &Expr {
    if let Expr::Block(ExprBlock {
        attrs,
        label,
        block,
        ..
    }) = value
    {
        if let ([], None, [Stmt::Expr(expr, None)]) =
            (attrs.as_slice(), label, block.stmts.as_slice())
        {
            return expr;
        }
    }
    value
}

/// Removes the `key` attribute, which wraps the element in a keyed element instead of being
/// passed on as an attribute or prop.
fn take_key(attributes: &mut Vec<NodeAttribute>) -> Option<Expr> {
    let index = attributes.iter().position(|attribute| {
        matches!(attribute, NodeAttribute::Attribute(attribute) if attribute.key.to_string() == "key")
    })?;
    match attributes.remove(index) {
        NodeAttribute::Attribute(attribute) => {
            Some(attribute_value(attribute.value().expect("Keys must have a value")).clone())
        }
        _ => unreachable!(),
    }
}

/// Wraps the element in a keyed element if it has a key, otherwise converts it into an element
fn into_element(element: TokenStream, key: Option<Expr>) -> TokenStream {
    match key {
        Some(key) => quote! {
            keyed(#key, #element)
        },
        None => quote! {
            #element.into()
        },
    }
}

fn transform_attributes(attributes: Vec<NodeAttribute>) -> TokenStream {
    let mut attrs = TokenStream::new();
    attributes
        .into_iter()
        .map(|attribute| match attribute {
            NodeAttribute::Attribute(attribute) => {
                let name = attribute.key.to_string();

                if name == "ref" {
                    let _value =
                        attribute_value(attribute.value().expect("Refs must be Arc<DomRef>"));
                    quote! {
                        .dom_ref(#_value)
                    }
                } else if let Some(event) = name.strip_prefix("on_") {
                    let _value =
                        attribute_value(attribute.value().expect("Callbacks must be functions"));
                    quote! {
                        .on(#event, #_value)
                    }
                } else if let Some(_value) = attribute.value().map(attribute_value) {
                    quote! {
                        .attr(#name, #_value)
                    }
                } else {
                    quote! {
                        .attr(#name, true)
                    }
                }
            }
            _ => panic!("not an attribute"),
        })
        .for_each(|attr| attrs.extend(attr));
    attrs
}

fn transform_props(attributes: Vec<NodeAttribute>) -> TokenStream {
    let mut props = TokenStream::new();
    attributes
        .into_iter()
        .map(|attribute| match attribute {
            NodeAttribute::Attribute(attribute) => {
                let name = &attribute.key;

                if let Some(value) = attribute.value().map(attribute_value) {
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

fn transform_component(
    tag: &ExprPath,
    generics: &TagGenerics,
    mut attributes: Vec<NodeAttribute>,
    children: Vec<Node>,
) -> TokenStream {
    let key = take_key(&mut attributes);
    let attributes = transform_props(attributes);
    let children = if children.is_empty() {
        TokenStream::new()
    } else {
        let children = transform_children(children);
        quote! {
            .children(#children)
        }
    };
    let generics = if generics.args.is_empty() {
        TokenStream::new()
    } else {
        quote!(::#generics)
    };
    into_element(
        quote! {
            #tag #generics ::builder() #children #attributes .build()
        },
        key,
    )
}

fn transform_tag(
    tag: &NodeName,
    mut attributes: Vec<NodeAttribute>,
    children: Vec<Node>,
) -> TokenStream {
    let key = take_key(&mut attributes);
    let attributes = transform_attributes(attributes);
    let tag = tag.to_string();
    let element = quote! {
        tag(#tag)#attributes.build()
    };
    if children.is_empty() {
        into_element(element, key)
    } else {
        let children = transform_children(children);
        let element = quote! {
            #element.children(#children)
        };
        match key {
            Some(_) => into_element(element, key),
            // .children() already returns an element
            None => element,
        }
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
            fn builder() -> Self {
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
            rstml::parse2(quote! { "hello world" })
                .unwrap()
                .into_iter()
                .next()
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
            rstml::parse2(quote! { <div ref={my_ref}></div> })
                .unwrap()
                .into_iter()
                .next()
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "tag (\"div\") . dom_ref (my_ref) . build () . into ()"
        );
    }

    #[test]
    fn render_component() {
        let actual = super::transform_node(
            rstml::parse2(quote! {
                <MyComponent number_prop=123 boolean_prop>
                  <div id="child" />
                </MyComponent>
            })
            .unwrap()
            .into_iter()
            .next()
            .unwrap(),
        );
        assert_eq!(actual.to_string(), "MyComponent :: builder () . children ({ let mut children = Vec :: with_capacity (1usize) ; children . push (tag (\"div\") . attr (\"id\" , \"child\") . build () . into ()) ; children . into () }) . number_prop (123) . boolean_prop (true) . build () . into ()")
    }

    #[test]
    fn render_generic_component() {
        let actual = super::transform_node(
            rstml::parse2(quote! {
                <MyComponent<i32> value=1 />
            })
            .unwrap()
            .into_iter()
            .next()
            .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "MyComponent :: < i32 > :: builder () . value (1) . build () . into ()"
        )
    }

    #[test]
    fn render_leaf_component() {
        let actual = super::transform_node(
            rstml::parse2(quote! {
                <MyComponent number_prop=123 boolean_prop label="test" />
            })
            .unwrap()
            .into_iter()
            .next()
            .unwrap(),
        );
        assert_eq!(actual.to_string(), "MyComponent :: builder () . number_prop (123) . boolean_prop (true) . label (\"test\") . build () . into ()")
    }

    #[test]
    fn keyed_tag() {
        let actual = super::transform_node(
            rstml::parse2(quote! { <li key={item.id} class="item" /> })
                .unwrap()
                .into_iter()
                .next()
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "keyed (item . id , tag (\"li\") . attr (\"class\" , \"item\") . build ())"
        );
    }

    #[test]
    fn keyed_tag_with_children() {
        let actual = super::transform_node(
            rstml::parse2(quote! { <li key={item.id}>"text"</li> })
                .unwrap()
                .into_iter()
                .next()
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "keyed (item . id , tag (\"li\") . build () . children ({ let mut children = Vec :: with_capacity (1usize) ; children . push (\"text\" . to_string () . into ()) ; children . into () }))"
        );
    }

    #[test]
    fn keyed_component() {
        let actual = super::transform_node(
            rstml::parse2(quote! { <Row key={item.id} label="test" /> })
                .unwrap()
                .into_iter()
                .next()
                .unwrap(),
        );
        assert_eq!(
            actual.to_string(),
            "keyed (item . id , Row :: builder () . label (\"test\") . build ())"
        );
    }
}
