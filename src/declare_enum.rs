use super::SymbolMap;
use rhai::{CustomType, Dynamic};
use serde::Deserialize;
use std::collections::HashSet;

use super::asset_shared::{AssetGroup, AssetId, AssetVal};
use super::registry::{AssetRegistration, Registry};

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
#[derive(Debug, Deserialize, Clone)]
pub struct DeclareEnumRaw {
    items: Vec<String>,
}

#[derive(Debug, Clone, CustomType)]
#[rhai_type(name = "DeclareEnum")]
pub struct DeclareEnumView {
    #[rhai_type(readonly)]
    pub items: rhai::Array,
}

#[derive(Debug, Clone, CustomType)]
#[rhai_type(name = "EnumFlags")]
pub struct EnumSelectionView {
    #[rhai_type(readonly)]
    pub reference: AssetId,
    #[rhai_type(readonly)]
    pub values: rhai::Array,
}

impl EnumSelectionView {
    #[doc(hidden)]
    pub fn __from_raw(raw: &EnumSelectionRaw) -> Self {
        Self {
            reference: raw.reference.clone(),
            values: raw.values.iter().cloned().map(Dynamic::from).collect(),
        }
    }
}

impl AssetGroup for DeclareEnum {
    const STRUCT_NAME: &'static str = "DeclareEnum";
    type Raw = DeclareEnumRaw;
    type View = DeclareEnumView;

    fn view_from_raw(raw: &Self::Raw, _registry: &Registry) -> Result<Option<Dynamic>, String> {
        Ok(Some(Dynamic::from(DeclareEnumView {
            items: raw.items.iter().cloned().map(Dynamic::from).collect(),
        })))
    }

    fn from_raw(
        raw: Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
        _asset_id_string: String,
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
    declaration: AssetVal<DeclareEnum>,
    bits: u32,
}

impl EnumFlags {
    #[doc(hidden)]
    pub fn __resolve(raw: EnumSelectionRaw, registry: &Registry) -> Result<Self, String> {
        let reference = raw.reference;
        let values = raw.values;

        let item = registry
            .lookup_item::<DeclareEnum>(&reference)
            .ok_or_else(|| format!("Missing Declare(Enum) asset {}", reference))?;
        let declaration = item.get_val();
        let definition = item.get();

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

    pub fn declaration_id(&self) -> AssetVal<DeclareEnum> {
        self.declaration
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    pub fn to_list(&self, registry: &Registry) -> Vec<String> {
        let declaration = registry
            .lookup_asset_by_id::<DeclareEnum>(self.declaration.get())
            .unwrap();
        declaration.mask_items(self.bits())
    }

    pub fn contains(&self, registry: &Registry, item: &str) -> bool {
        let declaration = registry
            .lookup_asset_by_id::<DeclareEnum>(self.declaration.get())
            .unwrap();
        declaration.contains(self.bits, item)
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct EnumSelectionRaw {
    #[serde(rename = "ref")]
    reference: AssetId,
    values: Vec<String>,
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<DeclareEnum>();
            registry.script_engine().build_type::<DeclareEnumView>();
            registry.script_engine().build_type::<EnumSelectionView>();
        }
    }
}
