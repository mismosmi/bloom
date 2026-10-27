use syn::ItemFn;

// Work in progress: not yet exposed as a macro.
#[allow(dead_code)]
pub(crate) fn transform_client_component(
    _attrs: proc_macro2::TokenStream,
    _item: ItemFn,
) -> proc_macro2::TokenStream {
    todo!("client components are not implemented yet")
}
