use super::structs;
use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;

/// Reject inputs that the generated implementation cannot support.
pub(crate) fn verify_struct(input: &syn::DeriveInput) -> syn::Result<()> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "MakeAsset does not support generic structs",
        ));
    }

    Ok(())
}

pub(crate) fn can_exclude_struct_rhai(input: &syn::DeriveInput) -> bool {
    for attr in &input.attrs {
        if attr.path().is_ident("exclude_from_rhai") {
            return true;
        }
    }
    false
}

pub(crate) fn parse_struct_name_attr(input: &syn::DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("struct_name") {
            if let Ok(lit_str) = attr.parse_args::<syn::LitStr>() {
                return Ok(lit_str.value().to_upper_camel_case());
            } else {
                return Err(syn::Error::new(
                    input.span(),
                    "#[struct_name(...)] requires a string literal",
                ));
            }
        }
    }

    // Default to snake_case struct name if not specified
    Ok(input.ident.to_string().to_upper_camel_case())
}

pub(crate) fn extract_named_fields(input: &syn::DeriveInput) -> syn::Result<Vec<&syn::Field>> {
    match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(named) => Ok(named.named.iter().collect()),
            _ => Err(syn::Error::new(
                input.span(),
                "Only named-field structs are supported",
            )),
        },
        _ => Err(syn::Error::new(input.span(), "Only structs are supported")),
    }
}

pub(crate) fn field_as_attr(field: &syn::Field, attr_str: &str) -> bool {
    field
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident(attr_str))
}

pub(crate) fn parse_container_and_inner(ty: &syn::Type) -> (structs::ContainerType, syn::Type) {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let container = if segment.ident == "Option" {
                Some(structs::ContainerType::Option)
            } else if segment.ident == "Vec" {
                Some(structs::ContainerType::Vec)
            } else {
                None
            };

            if let Some(container) = container {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if args.args.len() == 1 {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return (container, inner.clone());
                        }
                    }
                }
            }
        }
    }
    (structs::ContainerType::Single, ty.clone())
}

pub(crate) fn asset_val_types(ty: &syn::Type) -> Option<(syn::Type, Option<syn::Type>)> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    let has_field_type = segment.ident == "AssetFieldVal";
    if !has_field_type && segment.ident != "AssetVal" && segment.ident != "AssetIdVal" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };

    let mut type_args = args.args.iter();
    let target = match type_args.next()? {
        syn::GenericArgument::Type(target) => target.clone(),
        _ => return None,
    };
    let field_type = if has_field_type {
        match type_args.next()? {
            syn::GenericArgument::Type(field_type) => Some(field_type.clone()),
            _ => return None,
        }
    } else {
        None
    };
    if type_args.next().is_some() {
        return None;
    }

    Some((target, field_type))
}

pub(crate) fn generate_raw(
    raw_name: &syn::Ident,
    raw_fields: impl Iterator<Item = TokenStream>,
) -> TokenStream {
    quote! {
        #[derive(
            ::sns_asset_manager::__macros::serde::Deserialize,
            ::sns_asset_manager::__macros::serde::Serialize
        )]
        #[doc(hidden)]
        pub struct #raw_name {
            #( #raw_fields, )*
        }
    }
}

pub(crate) fn generate_view(
    view_name: &syn::Ident,
    struct_name: &str,
    view_fields: impl Iterator<Item = TokenStream>,
    exclude_struct: bool,
) -> TokenStream {
    if exclude_struct {
        quote! {}
    } else {
        quote! {
            #[derive(Clone,::sns_asset_manager::__macros::rhai::CustomType)]
            #[rhai_type(name = #struct_name)]
            #[doc(hidden)]
            pub struct #view_name {
                #( #view_fields, )*
            }
        }
    }
}

pub(crate) fn is_copy_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(_) => true,
        syn::Type::Tuple(tuple) => tuple.elems.iter().all(is_copy_type),
        _ => false,
    }
}

pub(crate) fn last_type_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    type_path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

pub(crate) fn iter_included(
    fields_info: &[structs::FieldInfo],
) -> impl Iterator<Item = &structs::FieldInfo> {
    fields_info.iter().filter(|f| f.is_rhai_included())
}

pub(crate) fn validate_view_type(view_name: syn::Ident, exclude_struct: bool) -> TokenStream {
    if exclude_struct {
        quote! { () }
    } else {
        quote! { #view_name }
    }
}

pub(crate) fn validate_view_impl(
    view_name: syn::Ident,
    view_values: impl Iterator<Item = TokenStream>,
    exclude_struct: bool,
) -> TokenStream {
    if exclude_struct {
        quote! {
                fn view_from_raw(
                        _raw: &Self::Raw,
                        _registry: &::sns_asset_manager::prelude::Registry,
                        _symbols: &std::collections::HashMap<String, u32>,
                ) -> Result<Option<::sns_asset_manager::__macros::rhai::Dynamic>, String> {
                        Ok(None)
                }
        }
    } else {
        quote! {
                fn view_from_raw(
                        raw: &Self::Raw,
                        registry: &::sns_asset_manager::prelude::Registry,
                        symbols: &std::collections::HashMap<String, u32>,
                ) -> Result<Option<::sns_asset_manager::__macros::rhai::Dynamic>, String> {
                        Ok(Some(::sns_asset_manager::__macros::rhai::Dynamic::from(
                                #view_name { #( #view_values, )* }
                        )))
                }
        }
    }
}

pub(crate) fn validate_reg_view(view_name: syn::Ident, exclude_struct: bool) -> TokenStream {
    if exclude_struct {
        quote! {}
    } else {
        quote! { registry.script_engine().build_type::<#view_name>(); }
    }
}

#[cfg(test)]
mod tests {
    use super::asset_val_types;
    use quote::quote;

    #[test]
    fn asset_field_val_parses_source_and_field_types() {
        let ty: syn::Type = syn::parse_quote!(AssetFieldVal<BasicA, Vec<bool>>);
        let (source, field) = asset_val_types(&ty).unwrap();

        assert_eq!(quote!(#source).to_string(), "BasicA");
        assert_eq!(quote!(#field).to_string(), "Vec < bool >");
    }

    #[test]
    fn asset_id_val_has_no_projected_field_type() {
        let ty: syn::Type = syn::parse_quote!(AssetIdVal<BasicA>);
        let (source, field) = asset_val_types(&ty).unwrap();

        assert_eq!(quote!(#source).to_string(), "BasicA");
        assert!(field.is_none());
    }
}
