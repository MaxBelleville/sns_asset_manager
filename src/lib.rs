use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::ops::Add;
use std::path::Path;
use vfs::{PhysicalFS, VfsPath, ZipFS};

use ron::extensions::Extensions;
use ron::options::Options;

mod assets;
mod registry;

use assets::*;

pub mod prelude {
    pub use super::assets::AssetId;
    pub use super::assets::AssetRef;
    pub use super::process_assets;
    pub use super::registry::AssetGroup;
    pub use super::registry::AssetRegistration;
    pub use super::registry::Registry;
    pub use super::registry::initialize_registry;
    pub use macros::MakeAsset;
}

pub type SymbolMap = HashMap<String, u32>;

pub fn process_assets(
    root: &Path,
    registry: &mut registry::Registry,
    use_current_dir: bool,
) -> Result<(), String> {
    let vfs_path = get_vfs_path(root, use_current_dir)?;
    let mut processed_files = HashSet::new();
    let mut collected_assets = HashMap::new();
    recursive_discovery(
        &mut processed_files,
        &mut collected_assets,
        vfs_path,
        AssetBundle::default(),
        true,
    )?;

    let (processed, symbols) = split_and_categorize_headers(collected_assets)?;
    for (struct_name, headers) in processed.statics {
        // 1. Fetch the correct loader for this specific struct type
        let loader = *registry.get_loader(&struct_name).ok_or_else(|| {
            format!(
                "No registered asset loader found for struct type: '{}'",
                struct_name
            )
        })?;

        for header in headers {
            // 2. Invoke only the correct loader with its pre-calculated ID and data
            loader(registry, header.asset_ref, &symbols, header.data.unwrap())?;
        }
    }

    Ok(())
}
///Get path from zip or a directory if zip.
///If a zip, return a ZipFS. If directory, return a PhysicalFS filesystem.
fn get_vfs_path(root: &Path, use_current_dir: bool) -> Result<VfsPath, String> {
    let mut path_buf = root.to_path_buf();

    if use_current_dir {
        let curr_dir = std::env::current_dir().unwrap();
        path_buf = curr_dir.join(path_buf);
    }
    if !path_buf.exists() {
        return Err(format!(
            "The root path does not exist: {}",
            path_buf.display()
        ));
    }

    if root.is_file() {
        if is_zip(root) {
            //If zip file, create a ZipFS from the file and return.
            return Ok(ZipFS::new(move || fs::File::open(&path_buf).unwrap()).into());
        }
        eprintln!(
            "Warning: File {} is not a zip file. Using parent directory instead.",
            root.display()
        );
        path_buf = root.parent().unwrap_or(root).to_path_buf();
    }
    if path_buf.is_dir() {
        Ok(PhysicalFS::new(path_buf).into())
    } else {
        return Err(format!(
            "Root path is not a directory: {}",
            path_buf.display()
        ));
    }
}

fn is_zip(root: &Path) -> bool {
    fs::File::open(&root)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .is_some()
}

fn split_and_categorize_headers(
    collected_assets: HashMap<String, AssetHeader>,
) -> Result<(ProcessedAssets, SymbolMap), String> {
    let mut processed = ProcessedAssets::default();
    let mut symbol_map = SymbolMap::new();
    let mut struct_counter = HashMap::<String, u32>::new();

    for (asset_ref, header) in collected_assets {
        if let Some(ref struct_name) = header.struct_name {
            let count = struct_counter.entry(struct_name.clone()).or_insert(0);
            *count += 1;
            symbol_map.insert(asset_ref, (*count).saturating_sub(1));
        }
        match header.asset_type {
            AssetType::Declare(_) => {
                let struct_name = header.struct_name.as_ref().unwrap();
                processed
                    .declares
                    .entry(struct_name.clone())
                    .or_default()
                    .push(header);
            }
            AssetType::Static => {
                let struct_name = header.struct_name.as_ref().unwrap();

                // Group into the HashMap by struct_name
                processed
                    .statics
                    .entry(struct_name.clone())
                    .or_default()
                    .push(header);
            }
            AssetType::Script => {
                // Scripts already had their IDs handled during parsing
                processed.scripts.push(header);
            }
        }
    }

    Ok((processed, symbol_map))
}

fn recursive_discovery(
    processed_files: &mut HashSet<String>,
    collected_assets: &mut HashMap<String, AssetHeader>,
    path: VfsPath,
    parent_bundle: AssetBundle,
    is_root: bool,
) -> Result<(), String> {
    let (bundle_paths, ron_paths, rhai_paths) = read_split_dir(&path);
    //Error checking
    // Check if
    if bundle_paths.len() != 1 && is_root {
        return Err(format!(
            "Expected exactly one bundle file in root directory"
        ));
    }

    if bundle_paths.len() > 1 {
        return Err(format!(
            "Multiple bundle files found in sub directory: {}",
            path.as_str()
        ));
    }

    if bundle_paths.is_empty() && ron_paths.is_empty() && rhai_paths.is_empty() {
        return Err(format!(
            "No bundle or asset files found in sub directory: {}",
            path.as_str()
        ));
    }

    let mut asset_bundle: AssetBundle = if bundle_paths.len() == 1 {
        parse_ron_file(&bundle_paths[0], "Failed to parse bundle file")?
    } else {
        AssetBundle::default()
    };

    combine_bundles(&mut asset_bundle, &parent_bundle)?;

    let mut asset_headers = Vec::new();
    let mut children = Vec::new();

    let mut add_asset_paths = false;

    if bundle_paths.is_empty() {
        add_asset_paths = true;
    }

    if asset_bundle.includes.is_empty() {
        add_asset_paths = true;
    } else {
        for include in &asset_bundle.includes {
            if include.contains("..") || Path::new(include).is_absolute() {
                return Err(format!(
                    "Include path '{}' attempts backtracking or absolute paths.",
                    include
                ));
            }

            let (is_bundle, is_ron, is_rhai) = (
                include.as_str().ends_with(".bundle.ron"),
                include.as_str().ends_with(".ron"),
                include.as_str().ends_with(".rhai"),
            );

            let mut include_path = path.join(include).map_err(|e| {
                format!(
                    "Failed to join include path {} with directory {}: {}",
                    include,
                    path.as_str(),
                    e
                )
            })?;

            if is_bundle {
                // If it's a bundle, set path to the folder it's in we we recursively load in later.
                include_path = include_path.parent();
            } else if !is_rhai && !is_ron && include_path.is_file().is_ok() {
                //If path doesn't end in ron or rhai extention, then we need to check if it's a file.
                Err(format!(
                    "Unknown file type for include: {}",
                    include_path.as_str()
                ))?;
            }

            let final_path_str: String = include_path.as_str().to_string();

            if is_ron || is_rhai {
                if processed_files.contains(&final_path_str) {
                    eprintln!(
                        "Warning duplicate path found {} in namespace {} skipping",
                        final_path_str, asset_bundle.full_namespace
                    );
                } else {
                    processed_files.insert(final_path_str);
                    let headers = gather_headers(
                        &mut *collected_assets,
                        include_path,
                        asset_bundle.full_namespace.clone(),
                        is_rhai,
                    )?;
                    asset_headers.extend(headers);
                }
            } else if include_path.is_dir().is_ok() {
                if include_path != path {
                    let child_node = recursive_discovery(
                        processed_files,
                        collected_assets,
                        include_path,
                        asset_bundle.clone(),
                        false,
                    )?;
                    children.push(child_node);
                } else {
                    add_asset_paths = true;
                }
            }
        }
    }
    if add_asset_paths {
        for asset_path in ron_paths.into_iter().chain(rhai_paths) {
            let final_path_str = asset_path.as_str().to_string();
            if processed_files.contains(&final_path_str) {
                eprintln!(
                    "Warning duplicate path found {} in namespace {} skipping",
                    final_path_str, asset_bundle.full_namespace
                );
            } else {
                let headers = gather_headers(
                    &mut *collected_assets,
                    asset_path,
                    asset_bundle.full_namespace.clone(),
                    final_path_str.ends_with(".rhai"),
                )?;
                processed_files.insert(final_path_str);
                asset_headers.extend(headers);
            }
        }
    }

    Ok(())
}

fn gather_headers(
    collected_assets: &mut HashMap<String, AssetHeader>,
    path: VfsPath,
    namespace: String,
    is_rhai: bool,
) -> Result<Vec<AssetHeader>, String> {
    let mut final_headers = Vec::new();

    let headers = if is_rhai {
        create_rhai_header(path, namespace)
    } else {
        create_ron_headers(path, namespace)?
    };
    for header in headers {
        let key: String = header.asset_ref.clone().into();
        if collected_assets.contains_key(&key) {
            eprintln!("Warning duplicate asset {} found skipping", key);
        } else {
            final_headers.push(header.clone());
            collected_assets.insert(key, header);
        }
    }
    Ok(final_headers)
}

/// Parses a .rhai file and returns a single script header
fn create_rhai_header(path: VfsPath, namespace: String) -> Vec<AssetHeader> {
    let script_name = path.filename().replace(".rhai", "");
    vec![AssetHeader::new_script(path, namespace, script_name)]
}

/// Parses a .ron file (which can be a single asset or a list) and returns headers
fn create_ron_headers(path: VfsPath, namespace: String) -> Result<Vec<AssetHeader>, String> {
    let raw_source: RawAssetSource = parse_ron_file(&path, "Failed to parse ron asset file")?;
    let path_name = path.filename().replace(".ron", "");

    let raw_headers = match raw_source {
        RawAssetSource::Single(raw) => vec![raw],
        RawAssetSource::List(list) => list,
    };

    let mut headers = Vec::new();

    for (i, raw) in raw_headers.into_iter().enumerate() {
        let mut full_path_name = path_name.clone();
        if i > 0 {
            full_path_name += &i.add(1).to_string();
        }
        headers.push(AssetHeader::new(
            raw,
            path.clone(),
            full_path_name,
            namespace.clone(),
        )?);
    }
    Ok(headers)
}

/// Read and split dir into 3 vectors, .bundle.ron, ron and rhai files
fn read_split_dir(path: &VfsPath) -> (Vec<VfsPath>, Vec<VfsPath>, Vec<VfsPath>) {
    let mut bundle_paths = Vec::new();
    let mut ron_paths = Vec::new();
    let mut rhai_paths = Vec::new();

    for file_paths in path.read_dir().unwrap() {
        if file_paths.as_str().ends_with(".bundle.ron") {
            bundle_paths.push(file_paths);
        } else if file_paths.as_str().ends_with(".ron") {
            ron_paths.push(file_paths);
        } else if file_paths.as_str().ends_with(".rhai") {
            rhai_paths.push(file_paths);
        }
    }
    (bundle_paths, ron_paths, rhai_paths)
}

fn parse_ron_file<T>(vfspath: &VfsPath, error_str: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let options = Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME | Extensions::UNWRAP_VARIANT_NEWTYPES);

    options
        .from_reader(vfspath.open_file().unwrap())
        .map_err(|e| format!("{} {}: {}", error_str, vfspath.as_str(), e))
}
///Reads in parent and current asset bundle, and appends namespace where necessary
fn combine_bundles(asset: &mut AssetBundle, parent: &AssetBundle) -> Result<(), String> {
    //Combine namespace
    if parent.namespace.is_none() && asset.namespace.is_some() {
        asset.full_namespace = asset.namespace.clone().unwrap();
    } else if parent.namespace.is_some() && asset.namespace.is_none() {
        asset.full_namespace = parent.full_namespace.clone();
    } else if parent.namespace.is_some() && asset.namespace.is_some() {
        asset.full_namespace = format!(
            "{}.{}",
            parent.full_namespace,
            asset.namespace.clone().unwrap()
        );
    } else {
        return Err("Both parent and child bundles have no namespace. Did you forget to add namespace to root bundle?".to_string());
    }

    //Combine version
    if parent.version.is_some() && asset.version.is_none() {
        asset.version = parent.version.clone();
    } else if parent.version.is_none() && asset.version.is_none() {
        return Err("No version specified. Did you include in the root?".to_string());
    }

    if parent.min_supported.is_some() && asset.min_supported.is_none() {
        asset.min_supported = parent.min_supported.clone();
    }

    Ok(())
}
