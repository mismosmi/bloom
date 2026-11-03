mod component;
mod identify;

use component::transform_component;
use syn::ItemFn;

#[proc_macro_derive(Identify)]
pub fn derive_identify(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    identify::derive_identify(syn::parse_macro_input!(item as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn component(
    attrs: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    transform_component(attrs.into(), syn::parse_macro_input!(item as ItemFn)).into()
}
