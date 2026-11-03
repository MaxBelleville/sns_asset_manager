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
    tag: AssetId<DeclareTag>,
}

pub fn main() {
    let mut registry = initialize_registry();

    let path = Path::new("./examples/assets/basic-all/");
    println!("current path {}", path.display());

    registry
        .script_engine()
        .register_fn("double", |value: i64| value * 2);

    process_assets(&path, &mut registry, false).map_err(|e| panic!("{}", e));

    let basic_enum = registry.lookup_asset::<BasicEnum>(0).unwrap();
    let basic_tag = registry.lookup_asset::<BasicFlag>(0).unwrap();

    let tagged_enums = basic_tag.tag.items::<BasicEnum>(&registry).unwrap();
    let tagged_declarations = basic_tag.tag.items::<DeclareEnum>(&registry).unwrap();
    println!(
        "Enum: {:?} Tag members: {:?} Declarations: {:?}",
        basic_enum.flags.to_list(&registry),
        tagged_enums,
        tagged_declarations,
    );
    let script_result = registry
        .call_by_ref(
            &AssetRef::from("core:script".to_owned()),
            "callback",
            vec![Dynamic::from(21_i64)],
        )
        .unwrap();
    println!("Script callback: {}", script_result);
}
