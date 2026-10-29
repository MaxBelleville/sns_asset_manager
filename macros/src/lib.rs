use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};
mod defined_asset;
mod static_asset;
mod utils;

#[proc_macro_derive(MakeAsset, attributes(struct_name))]
pub fn derive_make_asset(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let type_ref = utils::parse_struct_name_attr(&input);
    let fields = utils::extract_named_fields(&input);
    let fields_info = utils::analyze_asset_fields(fields);

    let name = &input.ident;
    let id_name = quote::format_ident!("{}ID", name);
    let raw_name = quote::format_ident!("{}Raw", name);
    static_asset::make(name, id_name, raw_name, type_ref, &fields_info).into()
}

#[proc_macro_derive(MakeDefinedAsset, attributes(struct_name))]
pub fn derive_make_derive_asset(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let type_ref = utils::parse_struct_name_attr(&input);
    let fields = utils::extract_named_fields(&input);
    let fields_info = utils::analyze_asset_fields(fields);

    let name = &input.ident;
    let id_name = quote::format_ident!("{}ID", name);
    let raw_name = quote::format_ident!("{}Raw", name);
    defined_asset::make(name, id_name, raw_name, type_ref, &fields_info).into()
}
