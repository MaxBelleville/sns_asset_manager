use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;

pub fn extract_named_fields(input: &syn::DeriveInput) -> syn::Result<Vec<&syn::Field>> {
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

pub fn is_copy_type(ty: &syn::Type) -> bool {
    match ty {
        // References are always Copy. Other types use a borrowed getter unless
        // the field explicitly opts in with `#[copy_getter]`.
        syn::Type::Reference(_) => true,
        // Keep recursive handling for tuples whose elements are themselves
        // known to be Copy (currently references).
        syn::Type::Tuple(tuple) => tuple.elems.iter().all(is_copy_type),
        _ => false,
    }
}

pub struct FieldInfo {
    pub name: syn::Ident,
    pub ty: syn::Type,
    pub asset_ty: Option<syn::Type>,
    pub container: ContainerType,
    pub copy_getter: bool,
}
pub enum ContainerType {
    Single,
    Option,
    Vec,
}

impl FieldInfo {
    pub fn raw_field(&self) -> TokenStream {
        let name = &self.name;
        let ty = &self.ty;
        if self.asset_ty.is_some() {
            return match self.container {
                ContainerType::Single => {
                    quote! { pub #name: ::sns_asset_manager::prelude::AssetRef }
                }
                ContainerType::Option => {
                    quote! { pub #name: Option<::sns_asset_manager::prelude::AssetRef> }
                }
                ContainerType::Vec => {
                    quote! { pub #name: Vec<::sns_asset_manager::prelude::AssetRef> }
                }
            };
        }

        quote! { pub #name: #ty }
    }
    pub fn convert_field(&self) -> TokenStream {
        let name = &self.name;
        let name_str = name.to_string();

        if let Some(target_ty) = &self.asset_ty {
            return match self.container {
                ContainerType::Single => quote! {
                    let #name = registry.resolve_asset_ref::<#target_ty>(&raw.#name,symbols, #name_str);
                },
                ContainerType::Option => quote! {
                    let #name = raw.#name.map(|r| {
                        registry.resolve_asset_ref::<#target_ty>(&r,symbols, #name_str)
                    });
                },
                ContainerType::Vec => quote! {
                    let #name = raw.#name
                        .into_iter()
                        .map(|r| registry.resolve_asset_ref::<#target_ty>(&r,symbols, #name_str))
                        .collect();
                },
            };
        }

        quote! {}
    }
    pub fn final_field(&self) -> TokenStream {
        let name = &self.name;

        if self.asset_ty.is_some() {
            return quote! { #name };
        }
        quote! { #name: raw.#name }
    }

    pub fn getter(&self) -> TokenStream {
        let name = &self.name;
        let ty = &self.ty;

        if self.copy_getter || is_copy_type(&self.ty) {
            return quote! {
                pub fn #name(&self) -> #ty {
                    self.#name
                }
            };
        }
        quote! {
            pub fn #name(&self) -> &#ty {
                &self.#name
            }
        }
    }
}

fn parse_container_and_inner(ty: &syn::Type) -> (ContainerType, syn::Type) {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let container = if segment.ident == "Option" {
                Some(ContainerType::Option)
            } else if segment.ident == "Vec" {
                Some(ContainerType::Vec)
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
    (ContainerType::Single, ty.clone())
}

pub fn analyze_asset_fields(fields: Vec<&syn::Field>) -> Vec<FieldInfo> {
    let mut result = Vec::new();

    for field in fields {
        let name = field.ident.clone().unwrap();
        let ty = field.ty.clone();

        let (container, inner_ty) = parse_container_and_inner(&ty);

        let mut asset_ty = None;

        if let syn::Type::Path(type_path) = &inner_ty {
            if let Some(segment) = type_path.path.segments.last() {
                if segment.ident == "AssetId" {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(target_type)) = args.args.first() {
                            asset_ty = Some(target_type.clone());
                        }
                    }
                }
            }
        }

        result.push(FieldInfo {
            name,
            ty: ty,
            asset_ty,
            container,
            copy_getter: field
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("copy_getter")),
        });
    }

    result
}

pub fn parse_struct_name_attr(input: &syn::DeriveInput) -> syn::Result<String> {
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

pub fn generate_raw(
    raw_name: &syn::Ident,
    raw_fields: impl Iterator<Item = TokenStream>,
) -> TokenStream {
    quote! {
        #[derive(::sns_asset_manager::__macros::serde::Deserialize)]
        struct #raw_name {
            #( #raw_fields, )*
        }
    }
}
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
