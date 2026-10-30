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
    fmt::{self, Display},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
};
use vfs::VfsPath;

/* "Pass 1" Asset Structs (Used in file io parsing, and asset organizing)
 *  Currently three major types are 1. Declare(internally defined assets that modify 'static' assets)
 *  ex: biome_flags Declare(Enum) with list of all available biome types.
 *  2. Static, externally defined Assets that hold custom values or references to other assets.
 *  ex: biome which includes weights for the chance of that biome, as well as a few biome_rules
 *  3. Script, a way to add simple custom functionality to asset or series of assets.
 */
#[derive(Debug)]
pub struct AssetId<T> {
    id: u32,
    _marker: std::marker::PhantomData<T>,
}
impl<T> Copy for AssetId<T> {}

impl<T> Clone for AssetId<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: PhantomData,
        }
    }
}

impl<T> From<u32> for AssetId<T> {
    fn from(value: u32) -> Self {
        Self {
            id: value,
            _marker: PhantomData,
        }
    }
}
impl<T> Display for AssetId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl<T> AssetId<T> {
    pub fn get_id(&self) -> u32 {
        return self.id;
    }
}

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
    pub(super) asset_name: Option<String>,
    pub(super) asset_type: AssetType,
    pub(super) struct_name: Option<String>,
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
            _ => raw.struct_name.unwrap().to_upper_camel_case(),
        };
        println!(
            "{}",
            AssetRef::new(namespace.clone(), name.clone()).to_string()
        );
        Ok(Self {
            path,
            asset_ref: AssetRef::new(namespace, name),
            asset_type: raw.asset_type,
            struct_name: Some(struct_name),
            data: Some(raw.data),
        })
    }
    pub fn new_script(path: VfsPath, path_name: String, namespace: String) -> Self {
        Self {
            path,
            asset_ref: AssetRef::new(namespace, path_name),
            asset_type: AssetType::Script,
            struct_name: None,
            data: None,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct AssetBundle {
    pub(crate) namespace: Option<String>,
    #[serde(skip)]
    pub(crate) full_namespace: String,
    pub(crate) version: Option<Version>,
    pub(crate) min_supported: Option<Version>,
    #[serde(default)]
    pub(crate) includes: Vec<String>,
    pub(crate) settings: Option<ron::Value>,
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

//"Shared" Assets structs (Structures that are helpful in both pass 1 and pass 2)

#[derive(Debug, Clone, Default)]
pub struct ProcessedAssets {
    pub(crate) declares: HashMap<String, Vec<AssetHeader>>,
    pub(crate) statics: HashMap<String, Vec<AssetHeader>>,
    pub(crate) scripts: Vec<AssetHeader>,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug, Default)]
pub struct AssetRef {
    pub namespace: Arc<str>,
    pub name: Arc<str>,
}
impl AssetRef {
    fn new(namespace: String, name: String) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}
impl From<String> for AssetRef {
    fn from(s: String) -> Self {
        let mut parts = s.split(':');
        Self {
            namespace: parts.next().unwrap_or("").into(),
            name: parts.next().unwrap_or("").into(),
        }
    }
}
impl Display for AssetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

impl Into<String> for AssetRef {
    fn into(self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }
}
impl<'de> Deserialize<'de> for AssetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let mut parts = s.split(':');
        let namespace = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("missing namespace"))?;
        let name = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("missing name"))?;
        Ok(AssetRef {
            namespace: namespace.into(),
            name: name.into(),
        })
    }
}
