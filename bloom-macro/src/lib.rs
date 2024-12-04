mod client_component;
mod component;

use component::transform_component;
use syn::ItemFn;

#[proc_macro_attribute]
pub fn component(
    attrs: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    transform_component(attrs.into(), syn::parse_macro_input!(item as ItemFn)).into()
}
