use super::SymbolMap;
use super::registry::Registry;
use rhai::{CustomType, Dynamic, TypeBuilder};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};

use std::{fmt::Display, hash::Hash, marker::PhantomData, sync::Arc};

pub trait AssetGroup: Sized + Send + Sync + 'static {
    const STRUCT_NAME: &'static str;
    type Raw: DeserializeOwned;
    type View: Clone + Send + Sync;

    fn view_from_raw(_raw: &Self::Raw, _registry: &Registry) -> Result<Option<Dynamic>, String> {
        Ok(None)
    }

    fn from_raw(
        raw: Self::Raw,
        registry: &mut Registry,
        symbols: &SymbolMap,
    ) -> Result<Self, String>;

    fn load_ron_asset(
        registry: &mut Registry,
        asset_ref: AssetRef,
        symbols: &SymbolMap,
        data: ron::Value,
    ) -> Result<(), String> {
        let asset_id_string = asset_ref
            .get_id()
            .ok_or_else(|| "Asset header reference is missing its asset ID".to_owned())?
            .to_string();
        let raw: Self::Raw = data
            .into_rust()
            .map_err(|error| format!("{} - {}: {}", Self::STRUCT_NAME, asset_id_string, error))?;

        let id = symbols.get(&asset_id_string).ok_or_else(|| {
            format!(
                "No assigned asset ID for {} ({})",
                Self::STRUCT_NAME,
                asset_id_string
            )
        })?;

        let view = Self::view_from_raw(&raw, registry)?;

        let asset = Self::from_raw(raw, registry, symbols)?;
        let view_ref = asset_ref.clone();

        let glossary = registry
            .__get_mut::<Self>()
            .ok_or_else(|| format!("Glossary not found for type: {}", Self::STRUCT_NAME))?;

        glossary.__insert_from_header(asset, asset_ref, *id)?;
        if let Some(view) = view {
            registry.script().insert_view(view_ref, view)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct AssetVal<T> {
    val: u32,
    _marker: std::marker::PhantomData<T>,
}
impl<T> Hash for AssetVal<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: std::hash::Hasher,
    {
        self.val.hash(state);
    }
}
impl<T> PartialEq for AssetVal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.val == other.val
    }
}

impl<T> Eq for AssetVal<T> {}

impl<T> Copy for AssetVal<T> {}

impl<T> Clone for AssetVal<T> {
    fn clone(&self) -> Self {
        Self {
            val: self.val,
            _marker: PhantomData,
        }
    }
}

impl<T> From<u32> for AssetVal<T> {
    fn from(value: u32) -> Self {
        Self {
            val: value,
            _marker: PhantomData,
        }
    }
}
impl<T> Display for AssetVal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.val)
    }
}

impl<T> AssetVal<T> {
    pub fn get(&self) -> u32 {
        return self.val;
    }
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct AssetRef {
    pub(crate) id: Option<AssetId>,
    pub(crate) collection: Option<CollectionId>,
}

impl AssetRef {
    pub(crate) fn new(namespace: String, name: String, collection_name: Option<String>) -> Self {
        Self {
            id: Some(AssetId::new(namespace, name)),
            collection: collection_name.map(|name| CollectionId::new(name)),
        }
    }

    pub(crate) fn new_id(namespace: String, name: String) -> Self {
        Self {
            id: Some(AssetId::new(namespace, name)),
            collection: None,
        }
    }

    pub fn get_id(&self) -> Option<&AssetId> {
        self.id.as_ref()
    }

    pub fn get_collection_id(&self) -> Option<&CollectionId> {
        self.collection.as_ref()
    }

    pub fn get_collection_name(&self) -> Option<&str> {
        self.collection.as_ref().map(CollectionId::get_name)
    }
}
impl Display for AssetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.id, &self.collection) {
            (Some(id), Some(collection)) => write!(f, "{} {}", id, collection),
            (Some(id), None) => write!(f, "{}", id),
            (None, Some(collection)) => write!(f, "{}", collection),
            (None, None) => f.write_str("<empty asset reference>"),
        }
    }
}

impl<'de> Deserialize<'de> for AssetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("#") {
            Ok(Self {
                id: None,
                collection: Some(
                    CollectionId::parse_selector(&s).map_err(serde::de::Error::custom)?,
                ),
            })
        } else if let Some((asset_id, collection)) = s.split_once(" #") {
            Ok(Self {
                id: Some(AssetId::parse(asset_id).map_err(serde::de::Error::custom)?),
                collection: Some(
                    CollectionId::parse_selector(&format!("#{}", collection))
                        .map_err(serde::de::Error::custom)?,
                ),
            })
        } else {
            Ok(Self {
                id: Some(AssetId::parse(&s).map_err(serde::de::Error::custom)?),
                collection: None,
            })
        }
    }
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct CollectionId(pub(crate) Arc<str>);

impl CollectionId {
    //used when processing asset header
    pub(crate) fn new(collection_name: String) -> Self {
        let collection = "#".to_string() + collection_name.as_str();
        Self(collection.into())
    }

    pub(crate) fn parse_selector(selector: &str) -> Result<Self, String> {
        let name = selector
            .strip_prefix('#')
            .ok_or_else(|| format!("Collection selector '{}' must start with '#'", selector))?;
        if name.is_empty() || name.contains(':') {
            return Err(format!(
                "Invalid collection selector '{}': expected #name",
                selector
            ));
        }
        Ok(Self::new(name.to_owned()))
    }

    pub fn get_name(&self) -> &str {
        self.0.strip_prefix('#').unwrap_or(&self.0)
    }
}

impl CustomType for CollectionId {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("CollectionId")
            .with_get("0", |collection_id: &mut Self| collection_id.0.to_string());
    }
}

impl Display for CollectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Hash, Eq, PartialEq, Clone, Debug, Default)]
pub struct AssetId {
    pub(crate) namespace: Arc<str>,
    pub(crate) name: Arc<str>,
}
impl AssetId {
    pub(crate) fn new(namespace: String, name: String) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let (namespace, name) = value
            .split_once(':')
            .ok_or_else(|| format!("Malformed asset ID '{}'; expected namespace:name", value))?;
        if namespace.is_empty()
            || name.is_empty()
            || name.contains(':')
            || namespace.contains('*')
            || namespace.contains('#')
            || name.contains('*')
        {
            return Err(format!(
                "Malformed asset ID '{}'; expected a valid namespace:name",
                value
            ));
        }
        Ok(Self::new(namespace.to_owned(), name.to_owned()))
    }
}

impl CustomType for AssetId {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("AssetId")
            .with_get("namespace", |asset_id: &mut Self| {
                asset_id.namespace.to_string()
            })
            .with_get("name", |asset_id: &mut Self| asset_id.name.to_string());
    }
}

impl From<String> for AssetId {
    fn from(s: String) -> Self {
        let mut parts = s.split(':');
        if parts.clone().count() != 2 {
            eprintln!("Warning: malformed asset ref {}", s)
        };
        Self {
            namespace: parts.next().unwrap_or("").into(),
            name: parts.next().unwrap_or("").into(),
        }
    }
}
impl Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

impl<'de> Deserialize<'de> for AssetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        AssetId::parse(&s).map_err(serde::de::Error::custom)
    }
}
