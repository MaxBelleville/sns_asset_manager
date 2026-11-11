use proc_macro::TokenStream;
use syn::{DeriveInput, Ident, parse_macro_input};
mod analysis;
mod structs;
mod utils;

use quote::quote;

#[proc_macro_derive(MakeAsset, attributes(struct_name, copy_getter, exclude_from_rhai))]
pub fn derive_make_asset(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let Err(error) = utils::verify_struct(&input) {
        return error.to_compile_error().into();
    }

    let type_ref_result = utils::parse_struct_name_attr(&input);
    let exclude_struct_rhai = utils::can_exclude_struct_rhai(&input);
    let fields_result: Result<Vec<&syn::Field>, syn::Error> = utils::extract_named_fields(&input);

    if let Err(error) = type_ref_result {
        return error.to_compile_error().into();
    }
    if let Err(error) = fields_result {
        return error.to_compile_error().into();
    }

    let (type_ref, fields) = (type_ref_result.unwrap(), fields_result.unwrap());

    let fields_info = analysis::asset_fields(fields);

    let name = &input.ident;

    make(name, type_ref, &fields_info, exclude_struct_rhai)
}

fn make(
    name: &Ident,
    struct_name: String,
    fields_info: &[structs::FieldInfo],
    exclude_struct: bool,
) -> TokenStream {
    let raw_name = quote::format_ident!("SNS{}Raw", name);
    let view_name = quote::format_ident!("SNS{}View", name);

    let raw_fields = fields_info.iter().map(|f| f.raw_field());

    let view_fields = utils::iter_included(fields_info).map(|f| f.view_field());
    let view_values = utils::iter_included(fields_info).map(|f| f.view_value());

    let convert_fields = fields_info.iter().map(|f| f.convert_field());
    let final_fields = fields_info.iter().map(|f| f.final_field());

    let getters = fields_info.iter().map(|f| f.getter());

    let raw_struct = utils::generate_raw(&raw_name, raw_fields);
    let view_struct = utils::generate_view(&view_name, &struct_name, view_fields, exclude_struct);

    let view_type = utils::validate_view_type(view_name.clone(), exclude_struct);
    let view_impl = utils::validate_view_impl(view_name.clone(), view_values, exclude_struct);
    let register_view = utils::validate_reg_view(view_name, exclude_struct);

    quote! {
        #raw_struct
        #view_struct

        impl #name {
          pub fn try_from_raw(
                raw: #raw_name,
                registry: &mut ::sns_asset_manager::prelude::Registry,
                symbols: &std::collections::HashMap<String,u32>,
                asset_id_string: String,
            ) -> Result<Self, String> {
              #( #convert_fields )*
              Ok(Self {
                  #( #final_fields, )*
              })
              }
              pub fn from_raw(
                raw: #raw_name,
                registry: &mut ::sns_asset_manager::prelude::Registry,
                symbols: &std::collections::HashMap<String,u32>,
                asset_id_string: String,
              ) -> Self {
                Self::try_from_raw(raw, registry, symbols, asset_id_string).expect("Failed to convert raw asset")
              }
              #( #getters )*

          }

          impl ::sns_asset_manager::__macros::AssetGroup for #name {
              const STRUCT_NAME: &'static str = #struct_name;
              type Raw = #raw_name;
              type View = #view_type;
              #view_impl
              fn from_raw(
                  raw: Self::Raw,
                  registry: &mut ::sns_asset_manager::prelude::Registry,
                  symbols: &std::collections::HashMap<String,u32>,
                  asset_id_string: String,
              ) -> Result<Self, String> {
                  Self::try_from_raw(raw, registry, symbols, asset_id_string)
              }
          }
          ::sns_asset_manager::__macros::inventory::submit! {
              ::sns_asset_manager::__macros::AssetRegistration {
                  register_fn: |registry| {
                      registry.__register::<#name>();
                      #register_view
                  }
              }
          };

    }
    .into()
}
