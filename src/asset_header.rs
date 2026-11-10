use super::asset_shared::{AssetId, AssetRef, CollectionId};
use heck::ToUpperCamelCase;
use semver::Version;
use serde::{
    Deserialize, Deserializer,
    de::{
        MapAccess, SeqAccess, Visitor,
        value::{MapAccessDeserializer, SeqAccessDeserializer},
    },
};
use std::{
    collections::HashMap,
    fmt::{self},
};
use vfs::VfsPath;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum DeclareType {
    Tag,
    Enum,
    //Add more types as needed
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum AssetType {
    Declare(DeclareType),
    Static,
    #[serde(skip)]
    Script,
}

impl DeclareType {
    fn as_str(&self) -> String {
        match self {
            DeclareType::Tag => "Tag".to_string(),
            DeclareType::Enum => "Enum".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum RawAssetSource {
    Single(RawAssetHeader),
    List(Vec<RawAssetHeader>),
}

impl<'de> Deserialize<'de> for RawAssetSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawAssetSourceVisitor;

        impl<'de> Visitor<'de> for RawAssetSourceVisitor {
            type Value = RawAssetSource;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an asset header or a list of asset headers")
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                RawAssetHeader::deserialize(MapAccessDeserializer::new(map))
                    .map(RawAssetSource::Single)
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                Vec::<RawAssetHeader>::deserialize(SeqAccessDeserializer::new(seq))
                    .map(RawAssetSource::List)
            }
        }

        deserializer.deserialize_any(RawAssetSourceVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawAssetHeader {
    #[serde(default)]
    pub(super) asset_name: Option<String>,
    pub(super) asset_type: AssetType,
    #[serde(default)]
    pub(super) struct_name: Option<String>,
    #[serde(default)]
    pub(super) collection_name: Option<String>,
    pub(super) data: ron::Value,
}

#[derive(Debug, Clone)]
pub struct AssetHeader {
    pub(crate) path: VfsPath,
    pub(crate) asset_ref: AssetRef,
    pub(crate) asset_type: AssetType,
    pub(crate) struct_name: Option<String>,
    pub(crate) data: Option<ron::Value>,
}

impl AssetHeader {
    pub fn new(
        raw: RawAssetHeader,
        path: VfsPath,
        path_name: String,
        namespace: String,
    ) -> Result<Self, String> {
        let name = if raw.asset_name.is_some() {
            raw.asset_name.unwrap()
        } else {
            path_name
        };
        Self::check_wildcard(
            name.clone(),
            namespace.clone(),
            raw.collection_name.as_deref(),
        )?;

        if matches!(&raw.asset_type, AssetType::Static) && raw.struct_name.is_none() {
            return Err(format!(
                "Error parsing {} 'Static' asset no struct name found in \"{}:{}\"",
                path.as_str(),
                namespace,
                name
            ));
        };

        let struct_name = match &raw.asset_type {
            AssetType::Declare(declare_type) => format!("Declare{}", declare_type.as_str()),
            AssetType::Static => raw.struct_name.unwrap().to_upper_camel_case(),
            _ => {
                return Err(format!(
                    "Tried to load rhai script via ron asset_header constructor: {}",
                    path.as_str()
                ));
            }
        };
        Ok(Self {
            path,
            asset_ref: AssetRef::new(namespace, name, raw.collection_name),
            asset_type: raw.asset_type,
            struct_name: Some(struct_name),
            data: Some(raw.data),
        })
    }
    pub fn new_script(path: VfsPath, path_name: String, namespace: String) -> Result<Self, String> {
        Self::check_wildcard(path_name.clone(), namespace.clone(), None)?;
        Ok(Self {
            path,
            asset_ref: AssetRef::new_id(namespace, path_name),
            asset_type: AssetType::Script,
            struct_name: Some("RhaiScript".to_owned()),
            data: None,
        })
    }
    fn check_wildcard(
        name: String,
        namespace: String,
        collection: Option<&str>,
    ) -> Result<(), String> {
        if name.contains('*') {
            return Err(format!(
                "Asset names cannot contain '*': {}:{}",
                namespace, name
            ));
        }
        if collection.is_some_and(|name| name.contains(':')) {
            return Err(format!(
                "Collection names cannot contain ':' for asset {}:{}",
                namespace, name
            ));
        }
        if namespace.contains('#') {
            return Err(format!(
                "Asset namespaces cannot contain '#': {}:{}",
                namespace, name
            ));
        }
        Ok(())
    }
    pub(crate) fn get_id(&self) -> Option<&AssetId> {
        self.asset_ref.get_id()
    }
    pub(crate) fn get_collection(&self) -> Option<&CollectionId> {
        self.asset_ref.get_collection_id()
    }
    pub(crate) fn check_struct(&self, struct_type: &str) -> bool {
        self.struct_name.as_deref() == Some(struct_type)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct AssetBundle {
    #[serde(default)]
    pub(crate) namespace: Option<String>,
    #[serde(skip)]
    pub(crate) full_namespace: String,
    #[serde(default)]
    pub(crate) version: Option<Version>,
    #[serde(default)]
    pub(crate) min_supported: Option<Version>,
    #[serde(default)]
    pub(crate) includes: Vec<String>,
    #[serde(default)]
    pub settings: Option<ron::Value>,
}
impl AssetBundle {
    pub fn get_namespace(&self) -> &str {
        self.full_namespace.as_str()
    }
    pub fn get_current_namespace(&self) -> &Option<String> {
        &self.namespace
    }
    pub fn get_version(&self) -> Version {
        self.version.clone().unwrap()
    }
    pub fn get_mininum_supported(&self) -> Version {
        self.min_supported.clone().unwrap()
    }
    pub fn get_settings(&self) -> &Option<ron::Value> {
        &self.settings
    }
}

#[derive(Debug, Clone)]
pub struct AssetBundleNode {
    pub(crate) path: VfsPath,
    pub(crate) bundle: AssetBundle,
    pub(crate) children: Vec<AssetBundleNode>,
}

impl AssetBundleNode {
    pub fn get_path(&self) -> &VfsPath {
        &self.path
    }
    pub fn get_bundle(&self) -> &AssetBundle {
        &self.bundle
    }
    /// Finds a sub-bundle node by its full namespace (e.g., "core.biomes")
    pub fn find_bundle(&self, target_namespace: &str) -> Option<&AssetBundleNode> {
        if self.bundle.full_namespace == target_namespace {
            return Some(self);
        }

        for child in &self.children {
            if let Some(node) = child.find_bundle(target_namespace) {
                return Some(node);
            }
        }

        None
    }
}

impl Default for AssetBundle {
    fn default() -> Self {
        AssetBundle {
            namespace: None,
            full_namespace: "".to_string(),
            version: None,
            min_supported: None,
            includes: Vec::new(),
            settings: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessedAssets {
    pub(crate) declares: HashMap<String, Vec<AssetHeader>>,
    pub(crate) statics: HashMap<String, Vec<AssetHeader>>,
    pub(crate) scripts: Vec<AssetHeader>,
}
