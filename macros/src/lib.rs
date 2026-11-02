use proc_macro::TokenStream;
use syn::{DeriveInput, Ident, parse_macro_input};
mod utils;

use quote::quote;

#[proc_macro_derive(MakeAsset, attributes(struct_name, copy_getter))]
pub fn derive_make_asset(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let Err(error) = utils::verify_struct(&input) {
        return error.to_compile_error().into();
    }

    let type_ref_result = utils::parse_struct_name_attr(&input);
    let fields_result = utils::extract_named_fields(&input);

    if let Err(error) = type_ref_result {
        return error.to_compile_error().into();
    }
    if let Err(error) = fields_result {
        return error.to_compile_error().into();
    }

    let (type_ref, fields) = (type_ref_result.unwrap(), fields_result.unwrap());

    let fields_info = utils::analyze_asset_fields(fields);

    let name = &input.ident;

    let raw_name = quote::format_ident!("SNS{}Raw", name);
    make(name, raw_name, type_ref, &fields_info)
}

fn make(
    name: &Ident,
    raw_name: Ident,
    struct_name: String,
    fields_info: &[utils::FieldInfo],
) -> TokenStream {
    let raw_fields = fields_info.iter().map(|f| f.raw_field());
    let convert_fields = fields_info.iter().map(|f| f.convert_field());
    let final_fields = fields_info.iter().map(|f| f.final_field());
    let getters = fields_info.iter().map(|f| f.getter());

    let raw_struct = utils::generate_raw(&raw_name, raw_fields);

    quote! {
        #raw_struct

        impl #name {
          pub fn try_from_raw(
                raw: #raw_name,
                registry: &mut ::sns_asset_manager::prelude::Registry,
                symbols: &std::collections::HashMap<String,u32>
            ) -> Result<Self, String> {
              #( #convert_fields )*
              Ok(Self {
                  #( #final_fields, )*
              })
              }
              pub fn from_raw(
                raw: #raw_name,
                registry: &mut ::sns_asset_manager::prelude::Registry,
                symbols: &std::collections::HashMap<String,u32>
              ) -> Self {
                Self::try_from_raw(raw, registry, symbols).expect("Failed to convert raw asset")
              }
              #( #getters )*

          }

          impl ::sns_asset_manager::__macros::AssetGroup for #name {
              const STRUCT_NAME: &'static str = #struct_name;
              type Raw = #raw_name;
              fn from_raw(
                  raw: Self::Raw,
                  registry: &mut ::sns_asset_manager::prelude::Registry,
                  symbols: &std::collections::HashMap<String,u32>,
              ) -> Result<Self, String> {
                  Self::try_from_raw(raw, registry, symbols)
              }
          }
          ::sns_asset_manager::__macros::inventory::submit! {
              ::sns_asset_manager::__macros::AssetRegistration {
                  register_fn: |registry| {
                      registry.__register::<#name>();
                  }
              }
          };

    }
    .into()
}
