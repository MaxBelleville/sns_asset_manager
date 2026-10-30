use std::path::Path;

use sns_asset_manager::prelude::*;

#[derive(Debug, MakeAsset)]
pub struct BasicA {
    is_loaded: bool,
}

#[derive(Debug, MakeAsset)]
pub struct BasicB {
    parent: Vec<AssetId<BasicA>>,
    is_loaded: bool,
}

pub fn main() {
    let mut registry = initialize_registry();
    let path = Path::new("./examples/assets/basic/");
    println!("current path {}", path.display());
    process_assets(&path, &mut registry, false).map_err(|e| panic!("{}", e));
    let basic = registry.lookup_asset::<BasicB>(0).unwrap();
    println!("is_loaded {}", basic.parent().get(0).unwrap())
}
