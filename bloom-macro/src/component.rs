use core::panic;

use quote::quote;
use syn::{AngleBracketedGenericArguments, GenericArgument, ItemFn, PathArguments, TypePath};

#[rustfmt::skip]
const RETURN_TYPE_ERROR_MSG: &str =
    "Component functions must return a Result<bloom_core::Element<Node, Error>, Error>";

pub(crate) fn transform_component(
    _attrs: proc_macro2::TokenStream,
    item: ItemFn,
) -> proc_macro2::TokenStream {
    let name = item.sig.ident;

    let mut fields = Vec::new();
    let mut field_aliases = Vec::new();

    for field in item.sig.inputs.iter() {
        let arg = if let syn::FnArg::Typed(pat) = field {
            pat
        } else {
            panic!("Expected typed argument")
        };

        let ident = if let syn::Pat::Ident(pat_ident) = arg.pat.as_ref() {
            pat_ident
        } else {
            panic!("Expected ident")
        };

        let ty = arg.ty.as_ref();

        field_aliases.push(quote! {
            let #ident = &self.#ident;
        });

        fields.push(quote! {
            #ident: #ty,
        });
    }

    let generics = item.sig.generics;
    let (impl_gen, ty_gen, where_gen) = generics.split_for_impl();
    let visibility = item.vis;
    let body = item.block;
    let return_type = item.sig.output;

    let (node_type, error_type) = match return_type {
        syn::ReturnType::Default => {
            panic!("{}", RETURN_TYPE_ERROR_MSG)
        }
        syn::ReturnType::Type(_arrow, return_type) => match *return_type {
            syn::Type::Path(TypePath { path, qself: _ }) => {
                let result = path
                    .segments
                    .into_iter()
                    .last()
                    .expect(RETURN_TYPE_ERROR_MSG);

                let (ok_type, err_type) =
                    if let PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        args,
                        ..
                    }) = result.arguments
                    {
                        let mut args = args.into_iter();
                        (args.next(), args.next())
                    } else {
                        panic!("{}", RETURN_TYPE_ERROR_MSG)
                    };

                let element_type =
                    if let Some(GenericArgument::Type(syn::Type::Path(type_path))) = ok_type {
                        type_path
                    } else {
                        panic!("{}", RETURN_TYPE_ERROR_MSG)
                    };

                let (node_type, element_error_type) =
                    if let PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        args,
                        ..
                    }) = element_type
                        .path
                        .segments
                        .into_iter()
                        .last()
                        .expect(RETURN_TYPE_ERROR_MSG)
                        .arguments
                    {
                        let mut args = args.into_iter();
                        (args.next(), args.next())
                    } else {
                        panic!("{}", RETURN_TYPE_ERROR_MSG)
                    };

                if element_error_type != err_type {
                    panic!("{}", RETURN_TYPE_ERROR_MSG)
                }

                if let (
                    Some(GenericArgument::Type(node_type)),
                    Some(GenericArgument::Type(error_type)),
                ) = (node_type, err_type)
                {
                    (node_type, error_type)
                } else {
                    panic!("{}", RETURN_TYPE_ERROR_MSG)
                }
            }
            _ => panic!("{}", RETURN_TYPE_ERROR_MSG),
        },
    };

    if fields.is_empty() {
        quote! {
            #[derive(PartialEq)]
            #visibility struct #name;

            impl #name {
                #visibility fn new() -> #name {
                    #name
                }

                #visibility fn build(self) -> #name {
                    self
                }
            }

            #[async_trait::async_trait]
            #visibility impl #impl_gen bloom_core::Component for #name #ty_gen #where_gen {
                type Node = #node_type;
                type Error = #error_type;

                async fn render(self: std::sync::Arc<Self>) -> Result<bloom_core::Element<Self::Node, Self::Error>, Self::Error> {
                    #(#field_aliases)*

                    #body
                }
            }
        }
    } else {
        quote! {
            #[derive(PartialEq, builder_pattern::Builder)]
            #visibility struct #name #ty_gen #where_gen {
                #(#[into] #fields)*
            }

            #[async_trait::async_trait]
            #visibility impl #impl_gen bloom_core::Component for #name #ty_gen #where_gen {
                type Node = #node_type;
                type Error = #error_type;

                async fn render(self: std::sync::Arc<Self>) -> Result<bloom_core::Element<Self::Node, Self::Error>, Self::Error> {
                    #(#field_aliases)*

                    #body
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;

    use super::*;

    #[test]
    fn basic_component() {
        let input = quote! {
            pub fn Foo(bar: String, baz: i32) -> Result<bloom_core::Element<bloom_html::HtmlNode, anyhow::Error>, anyhow::Error> {
                format!("{} times {}", bar, baz)
            }
        };

        let item = syn::parse2(input).expect("Failed to parse");

        let output = transform_component(TokenStream::new(), item);
        println!("{}", output);
    }
}
