use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Index, parse_quote};

pub(crate) fn derive_identify(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let mut generics = input.generics.clone();
    let fields: Vec<_> = match &input.data {
        Data::Struct(data) => data.fields.iter().collect(),
        Data::Enum(data) => data.variants.iter().flat_map(|v| v.fields.iter()).collect(),
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Identify can only be derived for structs and enums",
            ));
        }
    };
    for field in fields {
        let ty = &field.ty;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: ::bloom_core::Identify));
    }

    let body = match &input.data {
        Data::Struct(data) => {
            let hashes = data.fields.iter().enumerate().map(|(index, field)| {
                let member = field
                    .ident
                    .clone()
                    .map(syn::Member::Named)
                    .unwrap_or_else(|| syn::Member::Unnamed(Index::from(index)));
                hash_id(quote!(&self.#member))
            });
            quote!(#(#hashes)*)
        }
        Data::Enum(data) if data.variants.is_empty() => quote!(match *self {}),
        Data::Enum(data) => {
            let arms = data.variants.iter().map(|variant| {
                let variant_name = &variant.ident;
                let bindings: Vec<_> = (0..variant.fields.len())
                    .map(|index| format_ident!("__field_{}", index))
                    .collect();
                let pattern = match &variant.fields {
                    Fields::Named(fields) => {
                        let names = fields.named.iter().map(|field| &field.ident);
                        quote!({ #(#names: #bindings),* })
                    }
                    Fields::Unnamed(_) => quote!((#(#bindings),*)),
                    Fields::Unit => quote!(),
                };
                let hashes = bindings.iter().map(|binding| hash_id(quote!(#binding)));
                quote!(Self::#variant_name #pattern => { #(#hashes)* })
            });
            quote! {
                ::std::hash::Hash::hash(&::std::mem::discriminant(self), &mut __hasher);
                match self { #(#arms),* }
            }
        }
        Data::Union(_) => unreachable!(),
    };
    let method_body = if matches!(&input.data, Data::Enum(data) if data.variants.is_empty()) {
        body
    } else {
        quote! {
            let mut __hasher = ::std::hash::DefaultHasher::new();
            #body
            ::std::hash::Hasher::finish(&__hasher)
        }
    };
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::bloom_core::Identify for #name #ty_generics #where_clause {
            fn id(&self) -> u64 {
                #method_body
            }
        }
    })
}

fn hash_id(value: TokenStream) -> TokenStream {
    quote! {
        ::std::hash::Hash::hash(&::bloom_core::Identify::id(#value), &mut __hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unions() {
        let error = derive_identify(parse_quote! {
            union Value { number: u64, bytes: [u8; 8] }
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Identify can only be derived for structs and enums"
        );
    }
}
