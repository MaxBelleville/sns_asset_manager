use crate::registry::{AssetGroup, AssetRegistration, Registry};
use crate::{AssetRef, SymbolMap};
use serde::Deserialize;
use std::collections::HashSet;

/// Runtime definition for a declared set of enum values.
#[derive(Debug, Clone)]
pub struct DeclareEnum {
    items: Vec<String>,
}

impl DeclareEnum {
    /// Names in their declared order. The index is the bit position.
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// Returns the bit index assigned to an item name.
    pub fn bit_index(&self, item: &str) -> Option<u32> {
        self.items
            .iter()
            .position(|value| value == item)
            .map(|index| index as u32)
    }

    pub fn mask_items(&self, mask: u32) -> Vec<String> {
        self.items
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1u32 << index) != 0)
            .map(|(_, item)| item.clone())
            .collect()
    }

    /// Tests whether a mask contains a named item.
    pub fn contains(&self, mask: u32, item: &str) -> bool {
        self.bit_index(item)
            .is_some_and(|bit| mask & (1u32 << bit) != 0)
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize)]
pub struct DeclareEnumRaw {
    items: Vec<String>,
}

impl AssetGroup for DeclareEnum {
    const STRUCT_NAME: &'static str = "DeclareEnum";
    type Raw = DeclareEnumRaw;

    fn from_raw(
        raw: Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
    ) -> Result<Self, String> {
        if raw.items.len() > 32 {
            return Err(format!(
                "Declare(Enum) has {} items; at most 32 are supported",
                raw.items.len()
            ));
        }
        let mut names = HashSet::new();
        for item in &raw.items {
            if !names.insert(item) {
                return Err(format!("Declare(Enum) contains duplicate item '{}'", item));
            }
        }
        Ok(Self { items: raw.items })
    }
}

/// A selected subset of one registered enum declaration.
#[derive(Debug, Clone, Copy)]
pub struct EnumFlags {
    declaration: crate::AssetId<DeclareEnum>,
    bits: u32,
}

impl EnumFlags {
    #[doc(hidden)]
    pub fn __resolve(raw: EnumSelectionRaw, registry: &Registry) -> Result<Self, String> {
        let reference = raw.reference;
        let values = raw.values;

        let declaration = registry
            .lookup_id::<DeclareEnum>(reference.clone())
            .ok_or_else(|| format!("Missing Declare(Enum) asset {}", reference))?;

        let definition = registry
            .lookup_asset::<DeclareEnum>(declaration.get())
            .ok_or_else(|| format!("Declare(Enum) asset {} is not loaded", reference))?;

        let mut bits = 0u32;
        let mut selected = HashSet::new();

        for value in values {
            if !selected.insert(value.clone()) {
                return Err(format!(
                    "Enum selection for {} repeats '{}'",
                    reference, value
                ));
            }
            let bit = definition
                .bit_index(&value)
                .ok_or_else(|| format!("Unknown enum item '{}' in {}", value, reference))?;
            bits |= 1u32 << bit;
        }
        Ok(Self { declaration, bits })
    }

    pub fn declaration_id(&self) -> crate::AssetId<DeclareEnum> {
        self.declaration
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    pub fn to_list(&self, registry: &Registry) -> Vec<String> {
        let declaration = registry
            .lookup_asset::<DeclareEnum>(self.declaration.get())
            .unwrap();
        declaration.mask_items(self.bits())
    }

    pub fn contains(&self, registry: &Registry, item: &str) -> bool {
        let declaration = registry
            .lookup_asset::<DeclareEnum>(self.declaration.get())
            .unwrap();
        declaration.contains(self.bits, item)
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize)]
pub struct EnumSelectionRaw {
    #[serde(rename = "ref")]
    reference: AssetRef,
    values: Vec<String>,
}

/// Raw form of `Tag("namespace:name")` in a static asset field.
#[doc(hidden)]
#[derive(Debug, Deserialize)]
pub struct TagSelectionRaw(pub AssetRef);

/// A tag declaration groups asset references for consumers to query.
#[derive(Debug, Clone)]
pub struct DeclareTag {
    items: Vec<AssetRef>,
}

impl DeclareTag {
    pub fn items(&self) -> &[AssetRef] {
        &self.items
    }

    pub fn contains(&self, asset_ref: &AssetRef) -> bool {
        self.items.contains(asset_ref)
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize)]
pub struct DeclareTagRaw {
    items: Vec<AssetRef>,
}

impl AssetGroup for DeclareTag {
    const STRUCT_NAME: &'static str = "DeclareTag";
    type Raw = DeclareTagRaw;

    fn from_raw(
        raw: Self::Raw,
        _registry: &mut Registry,
        symbols: &SymbolMap,
    ) -> Result<Self, String> {
        let mut members = HashSet::new();
        for item in &raw.items {
            if !symbols.contains_key(&item.to_string()) {
                return Err(format!("Declare(Tag) references missing asset {}", item));
            }
            if !members.insert(item) {
                return Err(format!("Declare(Tag) contains duplicate asset {}", item));
            }
        }
        Ok(Self { items: raw.items })
    }
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<DeclareEnum>();
        }
    }
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<DeclareTag>();
        }
    }
}
