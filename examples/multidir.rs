use std::path::Path;

use sns_asset_manager::prelude::*;

#[derive(Debug, MakeAsset)]
pub struct BasicA {
    is_loaded: bool,
}

#[derive(Debug, MakeAsset)]
pub struct BasicB {
    #[exclude_from_rhai]
    parent: Vec<AssetVal<BasicA>>,
    is_loaded: bool,
}

pub fn main() {
    let mut registry = initialize_registry();
    let path = Path::new("./examples/assets/multidir/");
    println!("current path {}", path.display());
    process_assets(&path, &mut registry, false).map_err(|e| panic!("{}", e));
    let basic = registry.lookup_asset_by_id::<BasicB>(0).unwrap();

    println!(
        "is_loaded {} parent {}",
        basic.is_loaded,
        basic.parent.get(0).unwrap()
    )
}
