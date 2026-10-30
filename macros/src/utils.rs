use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::quote;

pub fn extract_named_fields(input: &syn::DeriveInput) -> Vec<&syn::Field> {
    match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(named) => named.named.iter().collect(),
            _ => panic!("Only named-field structs are supported"),
        },
        _ => panic!("Only structs are supported"),
    }
}

pub fn is_copy_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => {
            let ident = &type_path.path.segments.last().unwrap().ident;

            matches!(
                ident.to_string().as_str(),
                // primitives
                "u8" | "u16" | "u32" | "u64" |
                "i8" | "i16" | "i32" | "i64" |
                "f32" | "f64" | "Vec3" | "Vec4" | "Vec2" |
                "bool" | "char" |

                // common small types
                "usize" | "isize"
            )
        }

        // tuples like (f32, f32)
        syn::Type::Tuple(tuple) => tuple.elems.iter().all(is_copy_type),

        // references are Copy
        syn::Type::Reference(_) => true,

        _ => false,
    }
}

pub struct FieldInfo {
    pub name: syn::Ident,
    pub ty: syn::Type,
    pub asset_ty: Option<syn::Type>,
    pub container: ContainerType,
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

        if is_copy_type(&self.ty) {
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
        if let Some(segment) = type_path.path.segments.first() {
            if segment.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return (ContainerType::Option, inner.clone());
                    }
                }
            } else if segment.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return (ContainerType::Vec, inner.clone());
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
        });
    }

    result
}

pub fn parse_struct_name_attr(input: &syn::DeriveInput) -> String {
    for attr in &input.attrs {
        if attr.path().is_ident("struct_name") {
            if let Ok(lit_str) = attr.parse_args::<syn::LitStr>() {
                return lit_str.value().to_upper_camel_case();
            } else {
                panic!("#[struct_name(...)] requires a string literal");
            }
        }
    }

    // Default to snake_case struct name if not specified
    input.ident.to_string().to_upper_camel_case()
}

pub fn generate_raw(
    raw_name: &syn::Ident,
    raw_fields: impl Iterator<Item = TokenStream>,
) -> TokenStream {
    quote! {
        #[derive(serde::Deserialize)]
        struct #raw_name {
            #( #raw_fields, )*
        }
    }
}
