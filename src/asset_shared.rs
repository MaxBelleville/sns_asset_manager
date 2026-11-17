use super::SymbolMap;
use super::registry::{ErasedRaw, Registry};
use rhai::{CustomType, Dynamic, TypeBuilder};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

use std::hash::{Hash, Hasher};
use std::{fmt::Debug, fmt::Display, marker::PhantomData, sync::Arc};

pub trait AssetGroup: Sized + Send + Sync + 'static {
    const STRUCT_NAME: &'static str;
    type Raw: DeserializeOwned + Serialize + Send + Sync;
    type View: Clone + Send + Sync;

    fn raw_field_value(raw: &ErasedRaw, field: &str) -> Result<ron::Value, String> {
        let raw = raw
            .downcast_ref::<Self::Raw>()
            .ok_or_else(|| format!("Raw asset type mismatch for {}", Self::STRUCT_NAME))?;
        Self::ron_from_field(raw, field)
    }

    fn view_from_raw(
        _raw: &Self::Raw,
        _registry: &Registry,
        _symbols: &SymbolMap,
    ) -> Result<Option<Dynamic>, String> {
        Ok(None)
    }

    fn from_raw(
        raw: &Self::Raw,
        registry: &mut Registry,
        symbols: &SymbolMap,
        asset_val_string: String,
    ) -> Result<Self, String>;

    fn ron_from_field(_raw: &Self::Raw, field: &str) -> Result<ron::Value, String> {
        Err(format!(
            "{} does not expose raw field '{}'",
            Self::STRUCT_NAME,
            field
        ))
    }

    fn load_raw(
        registry: &mut Registry,
        asset_ref: &AssetRef,
        data: ron::Value,
    ) -> Result<(), String> {
        let asset_val_string = asset_ref
            .get_id()
            .ok_or_else(|| "Asset header reference is missing its asset ID".to_owned())?
            .to_string();

        let raw: Self::Raw = data
            .into_rust()
            .map_err(|error| format!("{} - {}: {}", Self::STRUCT_NAME, asset_val_string, error))?;

        registry.__add_raw(asset_val_string, Arc::new(raw));

        Ok(())
    }

    fn get_field<T: DeserializeOwned>(
        field_str: &str,
        erased_raw: &ErasedRaw,
    ) -> Result<T, String> {
        let Some(raw) = erased_raw.downcast_ref::<Self::Raw>() else {
            return Err(format!("Raw asset type mismatch for {}", Self::STRUCT_NAME));
        };
        let field = Self::ron_from_field(raw, field_str)?;
        let val: T = field
            .into_rust()
            .map_err(|error| format!("{} - {}: {}", Self::STRUCT_NAME, field_str, error))?;
        Ok(val)
    }

    fn load_ron_asset(
        registry: &mut Registry,
        asset_ref: AssetRef,
        symbols: &SymbolMap,
        erased_raw: &ErasedRaw,
    ) -> Result<(), String> {
        let asset_val_string = asset_ref.get_id().unwrap().to_string();

        let Some(raw) = erased_raw.downcast_ref::<Self::Raw>() else {
            return Err(format!("Raw asset type mismatch for {}", Self::STRUCT_NAME));
        };

        let id = symbols.get(&asset_val_string).ok_or_else(|| {
            format!(
                "No assigned asset ID for {} ({})",
                Self::STRUCT_NAME,
                asset_val_string
            )
        })?;

        let view = Self::view_from_raw(raw, registry, symbols)?;

        let asset = Self::from_raw(raw, registry, symbols, asset_val_string)?;

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
pub struct AssetFieldVal<I, T> {
    field: T,
    id: u32,
    _marker: std::marker::PhantomData<I>,
}
impl<I, T> Hash for AssetFieldVal<I, T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.id.hash(state);
    }
}
impl<I, T> PartialEq for AssetFieldVal<I, T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<I, T> Eq for AssetFieldVal<I, T> {}

impl<I, T: Copy> Copy for AssetFieldVal<I, T> {}

impl<I, T: Copy> Clone for AssetFieldVal<I, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I, T> Ord for AssetFieldVal<I, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl<I, T> PartialOrd for AssetFieldVal<I, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl<I, T: Debug> Display for AssetFieldVal<I, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {:?}", self.id, self.field)
    }
}

impl<I, T: Copy> AssetFieldVal<I, T> {
    pub fn get(&self) -> T {
        self.field
    }
}

impl<I, T> AssetFieldVal<I, T> {
    pub fn get_ref(&self) -> &T {
        &self.field
    }

    pub fn get_id(&self) -> u32 {
        self.id
    }
}

impl<I, T> From<(u32, T)> for AssetFieldVal<I, T> {
    fn from(val: (u32, T)) -> Self {
        Self {
            id: val.0,
            field: val.1,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug, Default)]
pub struct AssetIdVal<T> {
    val: u32,
    _marker: std::marker::PhantomData<T>,
}
impl<T> Hash for AssetIdVal<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.val.hash(state);
    }
}
impl<T> PartialEq for AssetIdVal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.val == other.val
    }
}

impl<T> Ord for AssetIdVal<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.val.cmp(&other.val)
    }
}

impl<T> PartialOrd for AssetIdVal<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.val.partial_cmp(&other.val)
    }
}

impl<T> Eq for AssetIdVal<T> {}

impl<T> Copy for AssetIdVal<T> {}

impl<T> Clone for AssetIdVal<T> {
    fn clone(&self) -> Self {
        Self {
            val: self.val,
            _marker: PhantomData,
        }
    }
}

impl<T> From<u32> for AssetIdVal<T> {
    fn from(value: u32) -> Self {
        Self {
            val: value,
            _marker: PhantomData,
        }
    }
}
impl<T> Display for AssetIdVal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.val)
    }
}

impl<T> AssetIdVal<T> {
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
    pub fn get_id_str(&self) -> String {
        self.id.as_ref().unwrap().to_string()
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

#[derive(Clone, Debug, Default)]
pub struct AssetId {
    pub(crate) namespace: Arc<str>,
    pub(crate) name: Arc<str>,
    pub(crate) field: Option<Arc<str>>,
}
impl AssetId {
    pub(crate) fn new(namespace: String, name: String) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            field: None,
        }
    }
    pub(crate) fn new_fielded(namespace: String, name: String, field: String) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            field: Some(field.into()),
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let (namespace, asset_and_field) = value
            .split_once(':')
            .ok_or_else(|| format!("Malformed asset ID '{}'; expected namespace:name", value))?;

        let (name, field) = match asset_and_field.split_once('.') {
            Some((name, field)) if !field.is_empty() && !field.contains('.') => (name, Some(field)),
            Some(_) => {
                return Err(format!(
                    "Malformed asset ID '{}'; expected namespace:asset[.field]",
                    value
                ));
            }
            None => (asset_and_field, None),
        };

        if namespace.is_empty()
            || name.is_empty()
            || asset_and_field.contains(':')
            || namespace.contains('*')
            || namespace.contains('#')
            || name.contains('*')
            || name.contains('.')
            || field.is_some_and(|field| {
                field.contains('*') || field.contains('#') || field.contains(':')
            })
        {
            return Err(format!(
                "Malformed asset ID '{}'; expected namespace:asset[.field] with no dots in asset names",
                value
            ));
        }

        Ok(match field {
            Some(field) => {
                Self::new_fielded(namespace.to_owned(), name.to_owned(), field.to_owned())
            }
            None => Self::new(namespace.to_owned(), name.to_owned()),
        })
    }

    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    pub(crate) fn get(&self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }
}

impl Hash for AssetId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.namespace.hash(state);
        self.name.hash(state);
    }
}

impl PartialEq for AssetId {
    fn eq(&self, other: &Self) -> bool {
        self.namespace == other.namespace && self.name == other.name
    }
}

impl Eq for AssetId {}

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
        match Self::parse(&s) {
            Ok(asset_id) => asset_id,
            Err(error) => {
                eprintln!("Warning: {}", error);
                match s.split_once(':') {
                    Some((namespace, name)) => Self::new(namespace.to_owned(), name.to_owned()),
                    None => Self::new(String::new(), s),
                }
            }
        }
    }
}
impl Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get())?;
        if let Some(field) = &self.field {
            write!(f, ".{}", field)?;
        }
        Ok(())
    }
}

impl Serialize for AssetId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
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
