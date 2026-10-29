use std::path::Path;

use sns_asset_manager::prelude::*;

#[derive(Debug, Default, MakeAsset)]
pub struct Basic {
    is_loaded: bool,
}

pub fn main() {
    let path = Path::new("./examples/assets/basic/");
    println!("current path {}", path.display());
    process_assets(&path, false).map_err(|e| panic!("{}", e));
}
