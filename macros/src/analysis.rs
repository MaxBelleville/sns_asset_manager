use super::{structs, utils};
use quote::quote;

pub(crate) fn asset_fields(fields: Vec<&syn::Field>) -> Vec<structs::FieldInfo> {
    let mut result = Vec::new();

    for field in fields {
        let name = field.ident.clone().unwrap();
        let ty = field.ty.clone();

        let (container, inner_ty) = utils::parse_container_and_inner(&ty);

        let processor = utils::asset_val_target(&inner_ty)
            .map(|target| asset_val_processor(target))
            .or_else(|| declared_field_processor(&inner_ty));

        result.push(structs::FieldInfo::new(
            name,
            ty,
            processor,
            container,
            utils::field_as_attr(field, "copy_getter"),
            utils::field_as_attr(field, "exclude_from_rhai"),
        ));
    }

    result
}

fn asset_val_processor(target: syn::Type) -> structs::FieldProcessor {
    let target_name = utils::last_type_name(&target);
    let definition = target_name.as_deref().and_then(asset_container_definition);
    match definition {
        Some(mut definition) => {
            definition.set_target(Some(target));
            definition
        }
        None => structs::FieldProcessor::new(
            quote! { ::sns_asset_manager::prelude::AssetId },
            quote! { ::sns_asset_manager::prelude::AssetId },
            Some(target),
            |target, value, field| {
                let target = target.expect("Asset reference processor requires a target type");
                quote! {
                    registry.resolve_asset_id::<#target>(&#value, symbols, #field)
                }
            },
            |value| quote! { (*#value).clone() },
            false,
            false,
            "",
        ),
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
    let definitions = [
        structs::FieldProcessor::new(
            quote! {::sns_asset_manager::__macros::TagSelectionRaw},
            quote! { ::sns_asset_manager::prelude::DeclareTagView },
            None,
            |target, value, field| {
                let target = target.expect("tag reference processor requires a target type");
                quote! {
                    registry.resolve_asset_id::<#target>(&#value.0, symbols, #field)
                }
            },
            |_value| {
                quote! {
                    ::sns_asset_manager::prelude::DeclareTagView::__resolve(#_value, registry)
                }
            },
            false,
            true,
            stringify!(DeclareTag),
        ),
        structs::FieldProcessor::new(
            quote! {::sns_asset_manager::__macros::EnumSelectionRaw},
            quote! { ::sns_asset_manager::prelude::EnumSelectionView },
            None,
            |_target, value, _field| {
                quote! {
                    ::sns_asset_manager::prelude::EnumFlags::__resolve(#value, registry)
                }
            },
            |value| {
                quote! {
                    ::sns_asset_manager::prelude::EnumSelectionView::__from_raw(#value)
                }
            },
            true,
            false,
            stringify!(EnumFlags),
        ),
    ];
    definitions
        .into_iter()
        .find(|definition| definition.get_rust_type() == rust_type)
}
