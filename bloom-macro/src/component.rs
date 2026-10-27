use core::panic;

use quote::{format_ident, quote};
use syn::{
    AngleBracketedGenericArguments, GenericArgument, GenericParam, ItemFn, PathArguments, TypePath,
};

#[rustfmt::skip]
const RETURN_TYPE_ERROR_MSG: &str =
    "Component functions must return a Result<bloom_core::Element<Node, Error>, Error>";

pub(crate) fn transform_component(
    _attrs: proc_macro2::TokenStream,
    item: ItemFn,
) -> proc_macro2::TokenStream {
    let name = item.sig.ident;

    let mut field_idents = Vec::new();
    let mut field_types = Vec::new();
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

        field_aliases.push(quote! {
            let #ident = &self.#ident;
        });

        field_idents.push(ident.ident.clone());
        field_types.push(arg.ty.as_ref().clone());
    }

    let generics = item.sig.generics;
    let (impl_gen, ty_gen, where_gen) = generics.split_for_impl();
    let visibility = item.vis;
    let body = item.block;
    let return_type = item.sig.output;

    let (node_type, error_type) = match return_type.clone() {
        syn::ReturnType::Default => {
            panic!("{}", RETURN_TYPE_ERROR_MSG)
        }
        syn::ReturnType::Type(_arrow, return_type) => match *return_type {
            syn::Type::Path(TypePath { path, .. }) => {
                let result = path
                    .segments
                    .into_iter()
                    .next_back()
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
                        .next_back()
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

    let component_impl = quote! {
        #[bloom_core::__private::async_trait]
        impl #impl_gen bloom_core::Component for #name #ty_gen #where_gen {
            type Node = #node_type;
            type Error = #error_type;

            async fn render(self: std::sync::Arc<Self>) #return_type {
                #(#field_aliases)*

                #body
            }
        }
    };

    if field_idents.is_empty() {
        return quote! {
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

            #component_impl
        };
    }

    // The builder tracks which props have been set in its type parameters,
    // so that build() is only available once every prop has been provided.
    let builder = format_ident!("{}Builder", name);
    let states: Vec<_> = (0..field_idents.len())
        .map(|index| format_ident!("__BloomProp{}", index))
        .collect();
    let user_params: Vec<_> = generics.params.iter().collect();
    let user_args: Vec<_> = generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Type(param) => {
                let ident = &param.ident;
                quote!(#ident)
            }
            GenericParam::Lifetime(param) => {
                let lifetime = &param.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Const(param) => {
                let ident = &param.ident;
                quote!(#ident)
            }
        })
        .collect();

    let setters = field_idents.iter().zip(&field_types).enumerate().map(|(index, (ident, ty))| {
        let other_states = states
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, state)| state);
        let before = states.iter().enumerate().map(|(other, state)| {
            if other == index { quote!(()) } else { quote!(#state) }
        });
        let after = states.iter().enumerate().map(|(other, state)| {
            if other == index { quote!(#ty) } else { quote!(#state) }
        });
        let values = field_idents.iter().enumerate().map(|(other, field)| {
            if other == index { quote!(#field: value.into()) } else { quote!(#field: self.#field) }
        });
        quote! {
            impl<#(#user_params,)* #(#other_states),*> #builder<#(#user_args,)* #(#before),*> #where_gen {
                #visibility fn #ident(self, value: impl Into<#ty>) -> #builder<#(#user_args,)* #(#after),*> {
                    #builder {
                        #(#values,)*
                        __bloom_component: ::core::marker::PhantomData,
                    }
                }
            }
        }
    });
    let unset = states.iter().map(|_| quote!(()));

    quote! {
        #[derive(PartialEq)]
        #visibility struct #name #ty_gen #where_gen {
            #(#field_idents: #field_types,)*
        }

        #[doc(hidden)]
        #visibility struct #builder<#(#user_params,)* #(#states),*> #where_gen {
            #(#field_idents: #states,)*
            __bloom_component: ::core::marker::PhantomData<fn() -> #name #ty_gen>,
        }

        impl #impl_gen #name #ty_gen #where_gen {
            #visibility fn new() -> #builder<#(#user_args,)* #(#unset),*> {
                #builder {
                    #(#field_idents: (),)*
                    __bloom_component: ::core::marker::PhantomData,
                }
            }
        }

        #(#setters)*

        impl #impl_gen #builder<#(#user_args,)* #(#field_types),*> #where_gen {
            #visibility fn build(self) -> #name #ty_gen {
                #name {
                    #(#field_idents: self.#field_idents,)*
                }
            }
        }

        #component_impl
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
