use super::utils;
use proc_macro2::TokenStream;
use quote::quote;

#[derive(Clone, Copy)]
pub(crate) enum ContainerType {
    Single,
    Option,
    Vec,
}

pub(crate) struct FieldProcessor {
    raw_type: TokenStream,
    view_type: TokenStream,
    target: Option<syn::Type>,
    convert_one: fn(Option<&syn::Type>, TokenStream, &str) -> TokenStream,
    view_convert: fn(TokenStream) -> TokenStream,
    fallible: bool,
    view_fallible: bool,
    rust_type: &'static str,
}

impl FieldProcessor {
    pub(crate) fn new(
        raw_type: TokenStream,
        view_type: TokenStream,
        target: Option<syn::Type>,
        convert_one: fn(Option<&syn::Type>, TokenStream, &str) -> TokenStream,
        view_convert: fn(TokenStream) -> TokenStream,
        fallible: bool,
        view_fallible: bool,
        rust_type: &'static str,
    ) -> Self {
        Self {
            raw_type,
            view_type,
            target,
            convert_one,
            view_convert,
            fallible,
            view_fallible,
            rust_type,
        }
    }

    pub(crate) fn set_target(&mut self, target: Option<syn::Type>) {
        self.target = target;
    }

    pub(crate) fn set_rust_type(&mut self, rust_type: &'static str) {
        self.rust_type = rust_type;
    }

    pub(crate) fn get_rust_type(&self) -> &'static str {
        self.rust_type
    }
}

pub(crate) struct FieldInfo {
    name: syn::Ident,
    ty: syn::Type,
    processor: Option<FieldProcessor>,
    container: ContainerType,
    copy_getter: bool,
    exclude_rhai: bool,
}

impl FieldInfo {
    pub(crate) fn new(
        name: syn::Ident,
        ty: syn::Type,
        processor: Option<FieldProcessor>,
        container: ContainerType,
        copy_getter: bool,
        exclude_rhai: bool,
    ) -> Self {
        Self {
            name,
            ty,
            processor,
            container,
            copy_getter,
            exclude_rhai,
        }
    }

    pub(crate) fn raw_field(&self) -> TokenStream {
        let name = &self.name;
        let ty = &self.ty;
        if let Some(processor) = &self.processor {
            let raw_ty_ref = &processor.raw_type;
            let raw_ty = match self.container {
                ContainerType::Single => quote! { #raw_ty_ref },
                ContainerType::Option => quote! { Option<#raw_ty_ref> },
                ContainerType::Vec => quote! { Vec<#raw_ty_ref> },
            };
            return quote! { pub #name: #raw_ty };
        }

        quote! { pub #name: #ty }
    }

    pub(crate) fn view_field(&self) -> TokenStream {
        let name = &self.name;
        let ty = &self.ty;
        if let Some(processor) = &self.processor {
            let view_ty_ref = &processor.view_type;
            let view_ty = match self.container {
                ContainerType::Single => quote! { #view_ty_ref },
                ContainerType::Option => quote! { Option<#view_ty_ref> },
                ContainerType::Vec => quote! { ::sns_asset_manager::__macros::rhai::Array },
            };
            return quote! { #[rhai_type(readonly)] pub #name: #view_ty };
        }

        if matches!(self.container, ContainerType::Vec) {
            return quote! {
                #[rhai_type(readonly)]
                pub #name: ::sns_asset_manager::__macros::rhai::Array
            };
        }
        quote! { #[rhai_type(readonly)] pub #name: #ty }
    }

    pub(crate) fn is_rhai_included(&self) -> bool {
        !self.exclude_rhai
    }

    pub(crate) fn view_value(&self) -> TokenStream {
        let name = &self.name;
        let Some(processor) = &self.processor else {
            return match self.container {
                ContainerType::Single | ContainerType::Option => {
                    quote! { #name: raw.#name.clone() }
                }
                ContainerType::Vec => quote! {
                    #name: raw.#name.iter().cloned()
                        .map(::sns_asset_manager::__macros::rhai::Dynamic::from)
                        .collect::<::sns_asset_manager::__macros::rhai::Array>()
                },
            };
        };

        let convert = |value| (processor.view_convert)(value);
        let field_name = name.to_string();
        match (self.container, processor.view_fallible) {
            (ContainerType::Single, false) => {
                let value = convert(quote! { &raw.#name });
                quote! { #name: #value }
            }
            (ContainerType::Single, true) => {
                let value = convert(quote! { &raw.#name });
                quote! {
                    #name: #value.map_err(|error| format!("Field '{}': {}", #field_name, error))?
                }
            }
            (ContainerType::Option, false) => {
                let value = convert(quote! { value });
                quote! { #name: raw.#name.as_ref().map(|value| #value) }
            }
            (ContainerType::Option, true) => {
                let value = convert(quote! { value });
                quote! {
                    #name: raw.#name.as_ref().map(|value| #value).transpose()
                        .map_err(|error| format!("Field '{}': {}", #field_name, error))?
                }
            }
            (ContainerType::Vec, false) => {
                let value = convert(quote! { value });
                quote! {
                    #name: raw.#name.iter().map(|value| {
                        ::sns_asset_manager::__macros::rhai::Dynamic::from(#value)
                    }).collect::<::sns_asset_manager::__macros::rhai::Array>()
                }
            }
            (ContainerType::Vec, true) => {
                let value = convert(quote! { value });
                quote! {
                    #name: raw.#name.iter().map(|value| {
                        #value.map(::sns_asset_manager::__macros::rhai::Dynamic::from)
                    }).collect::<Result<::sns_asset_manager::__macros::rhai::Array, _>>()
                        .map_err(|error| format!("Field '{}': {}", #field_name, error))?
                }
            }
        }
    }

    pub(crate) fn convert_field(&self) -> TokenStream {
        let name = &self.name;
        let field_name = name.to_string();

        let Some(processor) = &self.processor else {
            return quote! {};
        };

        let convert_one =
            |value| (processor.convert_one)(processor.target.as_ref(), value, &field_name);

        match (self.container, processor.fallible) {
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

        if self.copy_getter || utils::is_copy_type(&self.ty) {
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
