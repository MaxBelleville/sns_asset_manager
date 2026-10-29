use proc_macro2::TokenStream;
use quote::quote;

use super::utils;
use syn::Ident;
pub fn make(
    name: &Ident,
    id_name: Ident,
    raw_name: Ident,
    type_ref: String,
    fields_info: &Vec<utils::FieldInfo>,
) -> TokenStream {
    let raw_fields = fields_info.iter().map(|f| f.raw_field());
    let convert_fields = fields_info.iter().map(|f| f.convert_field());
    let final_fields = fields_info.iter().map(|f| f.final_field());
    let getters = fields_info.iter().map(|f| f.getter());

    let id_impl = utils::generate_id(&id_name);
    let raw_struct = utils::generate_raw(&raw_name, raw_fields);
    quote! {
        #id_impl
        #raw_struct
        impl ::sns_asset_manager::prelude::StaticAsset for #name {
          type ID = #id_name;
          const TYPE_REF: &'static str = #type_ref;
          fn load_asset(
              registries: &mut ::sns_asset_manager::prelude::RegistrySystem,
              defs: &::sns_asset_manager::prelude::DefinedRegistrySystem,
              value: ron::Value
          )->Result<(),String> {
              let raw: #raw_name = value.into_rust()
                  .map_err(|e| format!("{}: {}", #name::TYPE_REF, e))?;

              let asset_ref = ::sns_asset_manager::prelude::AssetRef::from(raw.id.clone());
              let asset = #name::from_raw(raw, registries);

              registries.insert::<#name>(asset, asset_ref);
              Ok(())

          }
        }
        impl #name {
          pub fn from_raw(
                raw: #raw_name,
                registries: &mut ::sns_asset_manager::prelude::RegistrySystem
            ) -> Self {
              #( #convert_fields )*
              Self {
                  #( #final_fields, )*
              }
              }
              #( #getters )*

          }
          pub fn create_storage() -> Box<dyn std::any::Any> {
              Box::new(
                  ::sns_asset_manager::prelude::AssetRegistry::<#id_name, #name>::default()
                )
             }
    }
}
