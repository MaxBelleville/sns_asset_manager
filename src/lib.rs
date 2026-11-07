use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use vfs::{PhysicalFS, VfsPath, ZipFS};

use ron::extensions::Extensions;
use ron::options::Options;

mod assets;
mod declares;
mod registry;
mod scripts;

#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use rhai;
#[doc(hidden)]
pub use ron;
#[doc(hidden)]
pub use serde;

#[cfg(feature = "bevy")]
use bevy_ecs::prelude::*;
#[cfg(feature = "bevy")]
#[derive(Resource)]
pub struct RegistryResource {
    registry: registry::Registry,
}
#[cfg(feature = "bevy")]
impl Default for RegistryResource {
    fn default() -> Self {
        Self {
            registry: registry::initialize_registry(),
        }
    }
}
#[cfg(feature = "bevy")]
impl RegistryResource {
    pub fn process_assets(
        &mut self,
        paths: Vec<String>,
        use_current_dir: bool,
    ) -> Result<Vec<assets::AssetBundleNode>, String> {
        let mut bundle_trees = Vec::new();
        for path in paths {
            let path = Path::new(&path);
            bundle_trees.push(process_assets(&path, &mut self.registry, use_current_dir)?);
        }
        Ok(bundle_trees)
    }
}

#[doc(hidden)]
pub mod __macros {
    pub use crate::declares::{EnumSelectionRaw, TagSelectionRaw};
    pub use crate::inventory;
    pub use crate::registry::{AssetGroup, AssetRegistration};
    pub use crate::rhai;
    pub use crate::serde;
}

pub mod prelude {
    pub use crate::assets::{AssetId, AssetRef};
    pub use crate::declares::{
        DeclareEnum, DeclareEnumView, DeclareTag, DeclareTagView, EnumFlags, EnumSelectionView,
    };
    #[cfg(feature = "bevy")]
    pub use crate::RegistryResource;
    #[cfg(not(feature = "bevy"))]
    pub use crate::process_assets;
    #[cfg(not(feature = "bevy"))]
    pub use crate::registry::initialize_registry;
    pub use crate::registry::{AssetGroup, GlossaryItem, Registry};
    pub use crate::rhai::Dynamic;
    pub use crate::scripts::{RhaiScript, ScriptRuntime};
    pub use macros::MakeAsset;
}

pub type SymbolMap = HashMap<String, u32>;
pub type PathGroups = (Vec<VfsPath>, Vec<VfsPath>, Vec<VfsPath>);

pub fn process_assets(
    root: &Path,
    registry: &mut registry::Registry,
    use_current_dir: bool,
) -> Result<assets::AssetBundleNode, String> {
    let vfs_path = get_vfs_path(root, use_current_dir)?;
    let mut processed_files = HashSet::new();
    let mut collected_assets = HashMap::new();
    let asset_bundles = recursive_discovery(
        &mut processed_files,
        &mut collected_assets,
        vfs_path,
        assets::AssetBundle::default(),
        true,
    )?;

    let (processed, symbols) = split_and_categorize_headers(collected_assets)?;

    // Declarations are loaded first because static fields may depend on them.
    for (struct_name, headers) in processed.declares {
        let loader = *registry.get_loader(&struct_name).ok_or_else(|| {
            format!(
                "No registered asset loader found for declaration type: '{}'",
                struct_name,
            )
        })?;
        for header in headers {
            loader(registry, header.asset_ref, &symbols, header.data.unwrap())?;
        }
    }

    for (struct_name, headers) in processed.statics {
        let loader = *registry.get_loader(&struct_name).ok_or_else(|| {
            format!(
                "No registered asset loader found for struct type: '{}'",
                struct_name
            )
        })?;
        for header in headers {
            loader(registry, header.asset_ref, &symbols, header.data.unwrap())?;
        }
    }

    for header in processed.scripts {
        registry.load_script(header, &symbols)?;
    }

    Ok(asset_bundles)
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

    if path_buf.is_file() {
        if is_zip(&path_buf) {
            //If zip file, create a ZipFS from the file and return.
            return Ok(ZipFS::new(move || fs::File::open(&path_buf).unwrap()).into());
        }
        eprintln!(
            "Warning: File {} is not a zip file. Using parent directory instead.",
            path_buf.display()
        );
        path_buf = path_buf.parent().unwrap_or(root).to_path_buf();
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

fn is_zip(path_buf: &PathBuf) -> bool {
    fs::File::open(&path_buf)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .is_some()
}

fn split_and_categorize_headers(
    collected_assets: HashMap<String, assets::AssetHeader>,
) -> Result<(assets::ProcessedAssets, SymbolMap), String> {
    let mut processed = assets::ProcessedAssets::default();

    let mut symbol_map = SymbolMap::new();
    //  let mut asset_types = HashMap::new();
    let mut struct_counter = HashMap::<String, u32>::new();

    let mut assets_vec: Vec<_> = collected_assets.into_iter().collect();
    assets_vec.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (asset_ref, header) in assets_vec {
        if let Some(ref struct_name) = header.struct_name {
            let count = struct_counter.entry(struct_name.clone()).or_insert(0);
            *count += 1;
            //   asset_types.insert(asset_ref.clone(), struct_name.clone());
            symbol_map.insert(asset_ref, (*count).saturating_sub(1));
        }
        match header.asset_type {
            assets::AssetType::Declare(_) => {
                let struct_name = header.struct_name.as_ref().unwrap();
                processed
                    .declares
                    .entry(struct_name.clone())
                    .or_default()
                    .push(header);
            }
            assets::AssetType::Static => {
                let struct_name = header.struct_name.as_ref().unwrap();

                // Group into the HashMap by struct_name
                processed
                    .statics
                    .entry(struct_name.clone())
                    .or_default()
                    .push(header);
            }
            assets::AssetType::Script => {
                processed.scripts.push(header);
            }
        }
    }

    Ok((processed, symbol_map))
}

fn recursive_discovery(
    processed_files: &mut HashSet<String>,
    collected_assets: &mut HashMap<String, assets::AssetHeader>,
    path: VfsPath,
    parent_bundle: assets::AssetBundle,
    is_root: bool,
) -> Result<assets::AssetBundleNode, String> {
    if path.is_dir().is_err() {
        return Err(format!("Path {} is not a directory", path.as_str()));
    }
    let (bundle_paths, ron_paths, rhai_paths) = read_split_dir(&path)?;
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

    let mut asset_bundle: assets::AssetBundle = if bundle_paths.len() == 1 {
        parse_ron_file(&bundle_paths[0], "Failed to parse bundle file")?
    } else {
        assets::AssetBundle::default()
    };

    combine_bundles(&mut asset_bundle, &parent_bundle)?;
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

            let normalized_include = include.trim_end_matches('/');
            let normalized_include = if normalized_include.is_empty() {
                include.as_str()
            } else {
                normalized_include
            };

            let (is_bundle, is_ron, is_rhai) = (
                normalized_include.ends_with(".bundle.ron"),
                normalized_include.ends_with(".ron"),
                normalized_include.ends_with(".rhai"),
            );

            println!("Path '{}' include '{}'", path.as_str(), include);

            let mut include_path = path.join(normalized_include).map_err(|e| {
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
            } else if !is_rhai && !is_ron && include_path.is_file().unwrap_or(false) {
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
                    gather_headers(
                        &mut *collected_assets,
                        include_path,
                        asset_bundle.full_namespace.clone(),
                        is_rhai,
                    )?;
                }
            } else if include_path.is_dir().unwrap_or(false) {
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
                gather_headers(
                    &mut *collected_assets,
                    asset_path,
                    asset_bundle.full_namespace.clone(),
                    final_path_str.ends_with(".rhai"),
                )?;
                processed_files.insert(final_path_str);
            }
        }
    }

    Ok(assets::AssetBundleNode {
        path,
        bundle: asset_bundle,
        children,
    })
}

fn gather_headers(
    collected_assets: &mut HashMap<String, assets::AssetHeader>,
    path: VfsPath,
    namespace: String,
    is_rhai: bool,
) -> Result<(), String> {
    let headers = if is_rhai {
        create_rhai_header(path, namespace)
    } else {
        create_ron_headers(path, namespace)?
    };
    for header in headers {
        let key: String = header.asset_ref.to_string();
        if collected_assets.contains_key(&key) {
            eprintln!("Warning duplicate asset {} found skipping", key);
        } else {
            collected_assets.insert(key, header);
        }
    }
    Ok(())
}

/// Parses a .rhai file and returns a single script header
fn create_rhai_header(path: VfsPath, namespace: String) -> Vec<assets::AssetHeader> {
    let script_name = path.filename().replace(".rhai", "");
    vec![assets::AssetHeader::new_script(
        path,
        script_name,
        namespace,
    )]
}

/// Parses a .ron file (which can be a single asset or a list) and returns headers
fn create_ron_headers(
    path: VfsPath,
    namespace: String,
) -> Result<Vec<assets::AssetHeader>, String> {
    let raw_source: assets::RawAssetSource =
        parse_ron_file(&path, "Failed to parse ron asset file")?;
    let path_name = path.filename().replace(".ron", "");

    let raw_headers = match raw_source {
        assets::RawAssetSource::Single(raw) => vec![raw],
        assets::RawAssetSource::List(list) => list,
    };

    let mut headers = Vec::new();

    for (i, raw) in raw_headers.into_iter().enumerate() {
        let mut full_path_name = path_name.clone();
        if i > 0 {
            full_path_name += &(i + 1).to_string();
        }
        headers.push(assets::AssetHeader::new(
            raw,
            path.clone(),
            full_path_name,
            namespace.clone(),
        )?);
    }
    Ok(headers)
}

/// Read and split dir into 3 vectors, .bundle.ron, ron and rhai files
fn read_split_dir(path: &VfsPath) -> Result<PathGroups, String> {
    let mut bundle_paths = Vec::new();
    let mut ron_paths = Vec::new();
    let mut rhai_paths = Vec::new();

    let Ok(file_paths_iter) = path.read_dir() else {
        return Err(format!("Could not read directory {}", path.as_str()));
    };

    for file_paths in file_paths_iter {
        if file_paths.as_str().ends_with(".bundle.ron") {
            bundle_paths.push(file_paths);
        } else if file_paths.as_str().ends_with(".ron") {
            ron_paths.push(file_paths);
        } else if file_paths.as_str().ends_with(".rhai") {
            rhai_paths.push(file_paths);
        }
    }
    Ok((bundle_paths, ron_paths, rhai_paths))
}

fn parse_ron_file<T>(vfspath: &VfsPath, error_str: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let options = Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME | Extensions::UNWRAP_VARIANT_NEWTYPES);

    let Ok(open_path) = vfspath.open_file() else {
        return Err(format!("Could not open path {}", vfspath.as_str()));
    };

    options
        .from_reader(open_path)
        .map_err(|e| format!("{} {}: {}", error_str, vfspath.as_str(), e))
}
///Reads in parent and current asset bundle, and appends namespace where necessary
fn combine_bundles(
    asset: &mut assets::AssetBundle,
    parent: &assets::AssetBundle,
) -> Result<(), String> {
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
        return Err("Both parent and child bundles have no namespace.
        Did you forget to add namespace to root bundle?"
            .to_string());
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
