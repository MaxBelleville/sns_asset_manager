use super::{structs, utils};
use quote::quote;

pub(crate) fn asset_fields(fields: Vec<&syn::Field>) -> syn::Result<Vec<structs::FieldInfo>> {
    let mut result = Vec::new();

    for field in fields {
        let name = field.ident.clone().unwrap();
        let ty = field.ty.clone();

        let (container, inner_ty) = utils::parse_container_and_inner(&ty);

        let processor = utils::asset_val_types(&inner_ty)
            .map(|(target, field_type)| asset_val_processor(target, field_type))
            .or_else(|| declared_field_processor(&inner_ty));

        let exclude_rhai = utils::field_as_attr(field, "exclude_from_rhai")
            || utils::last_type_name(&inner_ty).as_deref() == Some("RhaiFnRef");

        result.push(structs::FieldInfo::new(
            name,
            ty,
            processor,
            container,
            utils::field_as_attr(field, "copy_getter"),
            exclude_rhai,
        ));
    }

    Ok(result)
}

fn asset_val_processor(
    target: syn::Type,
    field_type: Option<syn::Type>,
) -> structs::FieldProcessor {
    let target_name = utils::last_type_name(&target);
    let definition = field_type
        .is_none()
        .then(|| target_name.as_deref().and_then(asset_container_definition))
        .flatten();
    match definition {
        Some(mut definition) => {
            definition.set_target(Some(target));
            definition
        }
        None => {
            let view_type = match &field_type {
                Some(field_type) => quote! { #field_type },
                None => quote! { ::sns_asset_manager::prelude::AssetId },
            };
            let view_convert = if field_type.is_some() {
                asset_field_view_convert
                    as fn(
                        Option<&syn::Type>,
                        Option<&syn::Type>,
                        proc_macro2::TokenStream,
                        &str,
                    ) -> proc_macro2::TokenStream
            } else {
                asset_id_view_convert
            };
            let view_fallible = field_type.is_some();
            structs::FieldProcessor::new(
                quote! { ::sns_asset_manager::prelude::AssetId },
                view_type,
                Some(target),
                field_type,
                |target, field_type, value, field| {
                    let target = target.expect("Asset reference processor requires a target type");
                    match field_type {
                        Some(field_type) => quote! {
                            registry.resolve_asset_field::<#target, #field_type>(
                                &#value,
                                symbols,
                                #field,
                            )
                        },
                        None => quote! {
                            registry.resolve_asset_id::<#target>(&#value, symbols, #field)
                        },
                    }
                },
                view_convert,
                true,
                view_fallible,
                "",
            )
        }
    }
}

fn asset_id_view_convert(
    _target: Option<&syn::Type>,
    _field_type: Option<&syn::Type>,
    value: proc_macro2::TokenStream,
    _field: &str,
) -> proc_macro2::TokenStream {
    quote! { (*#value).clone() }
}

fn asset_field_view_convert(
    target: Option<&syn::Type>,
    field_type: Option<&syn::Type>,
    value: proc_macro2::TokenStream,
    field: &str,
) -> proc_macro2::TokenStream {
    let target = target.expect("AssetFieldVal requires a source asset type");
    let field_type = field_type.expect("AssetFieldVal requires a projected field type");
    quote! {
        registry
            .resolve_asset_field::<#target, #field_type>(#value, symbols, #field)
            .map(|field_value| field_value.get())
    }
}

fn declared_field_processor(ty: &syn::Type) -> Option<structs::FieldProcessor> {
    let name = utils::last_type_name(ty)?;
    let mut definition = asset_container_definition(&name)?;
    definition.set_rust_type("");
    definition.set_target(None);
    Some(definition)
}

fn asset_container_definition(rust_type: &str) -> Option<structs::FieldProcessor> {
    // Add declared/raw field forms here; the shared FieldInfo path handles their
    // Single, Option, and Vec containers and applies the selected converter.
    let definitions: [structs::FieldProcessor; 3] = [
        structs::FieldProcessor::new(
            quote! {::sns_asset_manager::__macros::TagSelectionRaw},
            quote! { ::sns_asset_manager::prelude::DeclareTagView },
            None,
            None,
            |target, _field_type, value, field| {
                let target = target.expect("tag reference processor requires a target type");
                quote! {
                    registry.resolve_asset_id::<#target>(&#value.0, symbols, #field)
                }
            },
            |_target, _field_type, _value, _field| {
                quote! {
                    ::sns_asset_manager::prelude::DeclareTagView::__resolve(#_value, registry)
                }
            },
            true,
            true,
            stringify!(DeclareTag),
        ),
        structs::FieldProcessor::new(
            quote! {::sns_asset_manager::__macros::EnumSelectionRaw},
            quote! { ::sns_asset_manager::prelude::EnumSelectionView },
            None,
            None,
            |_target, _field_type, value, _field| {
                quote! {
                    ::sns_asset_manager::prelude::EnumFlags::__resolve((#value).clone(), registry)
                }
            },
            |_target, _field_type, value, _field| {
                quote! {
                    ::sns_asset_manager::prelude::EnumSelectionView::__from_raw(#value)
                }
            },
            true,
            false,
            stringify!(EnumFlags),
        ),
        structs::FieldProcessor::new(
            quote! {::sns_asset_manager::__macros::RhaiFnRefRaw},
            quote! { ::sns_asset_manager::prelude::AssetId },
            None,
            None,
            |_target, _field_type, value, _field| {
                quote! { registry.resolve_rhai_fn_ref((#value).clone(), asset_id_string) }
            },
            |_target, _field_type, value, _field| quote! { (#value).script().clone() },
            true,
            false,
            stringify!(RhaiFnRef),
        ),
    ];
    definitions
        .into_iter()
        .find(|definition| definition.get_rust_type() == rust_type)
}
