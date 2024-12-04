use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

use crate::component::transform_component;

pub(crate) fn transform_client_component(
    _attrs: proc_macro2::TokenStream,
    item: ItemFn,
) -> proc_macro2::TokenStream {
}
