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

pub(crate) fn analyze_asset_fields(fields: Vec<&syn::Field>) -> Vec<FieldInfo> {
    let mut result = Vec::new();

    for field in fields {
        let name = field.ident.clone().unwrap();
        let ty = field.ty.clone();

        let (container, inner_ty) = parse_container_and_inner(&ty);

        let processor = asset_id_target(&inner_ty)
            .map(|target| asset_id_processor(target))
            .or_else(|| declared_field_processor(&inner_ty));

        result.push(FieldInfo {
            name,
            ty: ty,
            processor,
            container,
            copy_getter: field
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("copy_getter")),
        });
    }

    result
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

fn asset_id_target(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "AssetId" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(target) => Some(target.clone()),
        _ => None,
    }
}

fn asset_id_processor(target: syn::Type) -> FieldProcessor {
    let target_name = last_type_name(&target);
    let definition = target_name.as_deref().and_then(asset_container_definition);
    match definition {
        Some(mut definition) => {
            definition.target = Some(target);
            definition
        }
        None => FieldProcessor {
            raw_type: quote! { ::sns_asset_manager::prelude::AssetRef },
            target: Some(target),
            convert_one: |target, value, field| {
                let target = target.expect("asset reference processor requires a target type");
                quote! {
                    registry.resolve_asset_ref::<#target>(&#value, symbols, #field)
                }
            },
            fallible: false,
            rust_type: "",
        },
    }
}

fn declared_field_processor(ty: &syn::Type) -> Option<FieldProcessor> {
    let name = last_type_name(ty)?;
    let definition = asset_container_definition(&name)?;
    Some(FieldProcessor {
        raw_type: definition.raw_type,
        target: None,
        convert_one: definition.convert_one,
        fallible: definition.fallible,
        rust_type: "",
    })
}

fn asset_container_definition(rust_type: &str) -> Option<FieldProcessor> {
    // Add declared/raw field forms here; the shared FieldInfo path handles their
    // Single, Option, and Vec containers and applies the selected converter.
    let definitions = [
        FieldProcessor {
            rust_type: stringify!(DeclareTag),
            raw_type: quote! {::sns_asset_manager::__macros::TagSelectionRaw},
            convert_one: |target, value, field| {
                let target = target.expect("tag reference processor requires a target type");
                quote! {
                    registry.resolve_asset_ref::<#target>(&#value.0, symbols, #field)
                }
            },
            fallible: false,
            target: None,
        },
        FieldProcessor {
            rust_type: stringify!(EnumFlags),
            raw_type: quote! {::sns_asset_manager::__macros::EnumSelectionRaw},
            convert_one: |_target, value, _field| {
                quote! {
                    ::sns_asset_manager::prelude::EnumFlags::__resolve(#value, registry)
                }
            },
            fallible: true,
            target: None,
        },
    ];
    definitions
        .into_iter()
        .find(|definition| definition.rust_type == rust_type)
}

pub(crate) fn generate_raw(
    raw_name: &syn::Ident,
    raw_fields: impl Iterator<Item = TokenStream>,
) -> TokenStream {
    quote! {
        #[derive(::sns_asset_manager::__macros::serde::Deserialize)]
        #[doc(hidden)]
        pub struct #raw_name {
            #( #raw_fields, )*
        }
    }
}

pub fn is_copy_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(_) => true,
        syn::Type::Tuple(tuple) => tuple.elems.iter().all(is_copy_type),
        _ => false,
    }
}

fn last_type_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    type_path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

pub(crate) struct FieldInfo {
    name: syn::Ident,
    ty: syn::Type,
    processor: Option<FieldProcessor>,
    container: ContainerType,
    copy_getter: bool,
}

impl FieldInfo {
    pub(crate) fn raw_field(&self) -> TokenStream {
        let name = &self.name;
        let ty = &self.ty;
        if let Some(processor) = &self.processor {
            let raw_ty = &processor.raw_type;
            let raw_ty = match self.container {
                ContainerType::Single => quote! { #raw_ty },
                ContainerType::Option => quote! { Option<#raw_ty> },
                ContainerType::Vec => quote! { Vec<#raw_ty> },
            };
            return quote! { pub #name: #raw_ty };
        }

        quote! { pub #name: #ty }
    }

    pub(crate) fn convert_field(&self) -> TokenStream {
        let Some(processor) = &self.processor else {
            return quote! {};
        };
        let name = &self.name;
        let field_name = name.to_string();
        let convert_one =
            |value| (processor.convert_one)(processor.target.as_ref(), value, &field_name);
        convert_container(
            self.container,
            name,
            &field_name,
            convert_one,
            processor.fallible,
        )
    }

    pub(crate) fn final_field(&self) -> TokenStream {
        let name = &self.name;

        if self.processor.is_some() {
            return quote! { #name };
        }
        quote! { #name: raw.#name }
    }

    pub(crate) fn getter(&self) -> TokenStream {
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

struct FieldProcessor {
    rust_type: &'static str,
    raw_type: TokenStream,
    target: Option<syn::Type>,
    convert_one: fn(Option<&syn::Type>, TokenStream, &str) -> TokenStream,
    fallible: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ContainerType {
    Single,
    Option,
    Vec,
}

fn convert_container(
    container: ContainerType,
    name: &syn::Ident,
    field_name: &str,
    convert_one: impl Fn(TokenStream) -> TokenStream,
    fallible: bool,
) -> TokenStream {
    match (container, fallible) {
        (ContainerType::Single, false) => {
            let expression = convert_one(quote! { raw.#name });
            quote! { let #name = #expression; }
        }
        (ContainerType::Single, true) => {
            let expression = convert_one(quote! { raw.#name });
            quote! {
                let #name = #expression
                    .map_err(|error| format!("Field '{}': {}", #field_name, error))?;
            }
        }
        (ContainerType::Option, false) => {
            let expression = convert_one(quote! { value });
            quote! { let #name = raw.#name.map(|value| #expression); }
        }
        (ContainerType::Option, true) => {
            let expression = convert_one(quote! { value });
            quote! {
                let #name = raw.#name.map(|value| #expression).transpose()
                    .map_err(|error| format!("Field '{}': {}", #field_name, error))?;
            }
        }
        (ContainerType::Vec, false) => {
            let expression = convert_one(quote! { value });
            quote! {
                let #name = raw.#name.into_iter().map(|value| #expression).collect();
            }
        }
        (ContainerType::Vec, true) => {
            let expression = convert_one(quote! { value });
            quote! {
                let #name = raw.#name.into_iter().map(|value| #expression)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("Field '{}': {}", #field_name, error))?;
            }
        }
    }
}
