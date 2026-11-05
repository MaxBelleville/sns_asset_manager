use super::registry::{AssetGroup, AssetRegistration, GlossaryItem, Registry};
use super::{SymbolMap, assets};
use rhai::{CustomType, Dynamic, Map};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

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
    pub reference: assets::AssetRef,
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
    declaration: assets::AssetId<DeclareEnum>,
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
        let declaration = assets::AssetId::<DeclareEnum>::from(item.get_id());
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

    pub fn declaration_id(&self) -> assets::AssetId<DeclareEnum> {
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
    reference: assets::AssetRef,
    values: Vec<String>,
}

/// Raw form of `Tag("namespace:name")` in a static asset field.
#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct TagSelectionRaw(pub assets::AssetRef);

/// A tag declaration groups asset references for consumers to query.
#[derive(Debug, Clone)]
pub struct DeclareTag {
    items: HashMap<String, Vec<assets::AssetRef>>,
}

impl DeclareTag {
    pub fn items(&self) -> &HashMap<String, Vec<assets::AssetRef>> {
        &self.items
    }

    pub fn contains(&self, asset_ref: &assets::AssetRef) -> bool {
        self.items.values().any(|items| items.contains(asset_ref))
    }

    pub fn items_for_type(&self, struct_name: &str) -> &[assets::AssetRef] {
        self.items
            .get(struct_name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct DeclareTagRaw {
    items: HashMap<String, Vec<assets::AssetRef>>,
}

#[derive(Debug, Clone, CustomType)]
#[rhai_type(name = "DeclareTag")]
pub struct DeclareTagView {
    #[rhai_type(readonly)]
    pub items: Map,
}

impl DeclareTagView {
    fn from_items(items: &HashMap<String, Vec<assets::AssetRef>>) -> Self {
        let items = items
            .iter()
            .map(|(asset_type, refs)| {
                let refs = refs
                    .iter()
                    .cloned()
                    .map(Dynamic::from)
                    .collect::<rhai::Array>();
                (asset_type.clone().into(), Dynamic::from(refs))
            })
            .collect();
        Self { items }
    }

    #[doc(hidden)]
    pub fn __resolve(raw: &TagSelectionRaw, registry: &Registry) -> Result<Self, String> {
        let item = registry
            .lookup_item::<DeclareTag>(&raw.0)
            .ok_or_else(|| format!("Missing Declare(Tag) asset {}", raw.0))?;
        Ok(Self::from_items(&item.get().items))
    }
}

impl AssetGroup for DeclareTag {
    const STRUCT_NAME: &'static str = "DeclareTag";
    type Raw = DeclareTagRaw;
    type View = DeclareTagView;

    fn view_from_raw(raw: &Self::Raw, _registry: &Registry) -> Result<Option<Dynamic>, String> {
        Ok(Some(Dynamic::from(DeclareTagView::from_items(&raw.items))))
    }

    fn from_raw(
        raw: Self::Raw,
        _registry: &mut Registry,
        symbols: &SymbolMap,
    ) -> Result<Self, String> {
        let mut members = HashSet::new();
        for item in raw.items.values().flatten() {
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

impl assets::AssetId<DeclareTag> {
    /// Resolves the members of this tag as typed glossary items.
    pub fn items<'a, A: AssetGroup>(
        &self,
        registry: &'a Registry,
    ) -> Result<Vec<&'a GlossaryItem<A>>, String> {
        let tag = registry
            .get_item::<DeclareTag>(self.get() as usize)
            .ok_or_else(|| format!("Declare(Tag) ID {} is not loaded", self.get()))?;

        tag.get()
            .items_for_type(A::STRUCT_NAME)
            .iter()
            .map(|asset_ref| {
                registry.lookup_item::<A>(asset_ref).ok_or_else(|| {
                    format!(
                        "Tag member {} is not loaded as type '{}'",
                        asset_ref,
                        A::STRUCT_NAME
                    )
                })
            })
            .collect()
    }
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

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<DeclareTag>();
            registry.script_engine().build_type::<DeclareTagView>();
        }
    }
}
