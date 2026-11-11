use std::path::Path;

use sns_asset_manager::prelude::*;

#[derive(Debug, MakeAsset)]
pub struct BasicEnum {
    is_loaded: bool,
    flags: EnumFlags,
}

#[derive(Debug, MakeAsset)]
pub struct BasicFlag {
    is_loaded: bool,
    tag: AssetVal<DeclareTag>,
    on_spawn: RhaiFnRef,
}

pub fn main() {
    let mut registry = initialize_registry();

    let path = Path::new("./examples/assets/basic-all/");
    println!("current path {}", path.display());

    process_assets(&path, &mut registry, false).map_err(|e| panic!("{}", e));

    let basic_enum = registry.lookup_asset_by_id::<BasicEnum>(0).unwrap();
    let basic_tag = registry.lookup_asset_by_id::<BasicFlag>(0).unwrap();

    let tagged_enums = basic_tag.tag.items::<BasicEnum>(&registry).unwrap();
    let tagged_declarations = basic_tag.tag.items::<DeclareEnum>(&registry).unwrap();
    println!(
        "Enum: {:?} Tag members: {:?} Declarations: {:?}",
        basic_enum.flags.to_list(&registry),
        registry.glossary_items_str(tagged_enums),
        registry.glossary_items_str(tagged_declarations),
    );

    let asset_callback_ref = basic_tag.on_spawn.clone();

    registry
        .script_engine()
        .register_fn("double", |value: i64| value * 2);

    let rhai_callback = registry
        .get_rhai_fn(&AssetId::from("core:script".to_string()), "callback")
        .unwrap();
    let script_result = rhai_callback.call(vec![Dynamic::from(21_i64)]).unwrap();
    let second_result = rhai_callback.call(vec![Dynamic::from(42_i64)]).unwrap();

    println!("Script callback: {}", script_result);
    println!("Script callback again: {}", second_result);

    let asset_callback = registry
        .call_fn_ref(&asset_callback_ref, vec![Dynamic::from(21_i64)])
        .unwrap();

    println!("Asset callback: {}", asset_callback);
}
