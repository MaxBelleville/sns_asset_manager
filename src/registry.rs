use rhai::{Dynamic, Engine};

use crate::asset_shared::AssetFieldVal;

use super::SymbolMap;
use super::asset_header::AssetHeader;
use super::asset_shared::{AssetGroup, AssetId, AssetIdVal, AssetRef};
use super::scripts::{
    RhaiFnRef, RhaiFnRefRaw, RhaiFunction, RhaiScript, ScriptRuntime, ViewLookup,
};

use serde::de::DeserializeOwned;

use std::fmt::{Debug, Display};

use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[cfg(feature = "bevy")]
use super::{asset_header::AssetBundleNode, process_assets};
#[cfg(feature = "bevy")]
use bevy_ecs::prelude::*;
#[cfg(feature = "bevy")]
use std::path::Path;

#[cfg(feature = "bevy")]
#[derive(Resource)]
pub struct RegistryResource {
    registry: Registry,
}
#[cfg(feature = "bevy")]
impl Default for RegistryResource {
    fn default() -> Self {
        Self {
            registry: initialize_registry(),
        }
    }
}
#[cfg(feature = "bevy")]
impl RegistryResource {
    pub fn process_assets(
        &mut self,
        paths: Vec<String>,
        use_current_dir: bool,
    ) -> Result<Vec<AssetBundleNode>, String> {
        let mut bundle_trees = Vec::new();
        for path in paths {
            let path = Path::new(&path);
            bundle_trees.push(process_assets(&path, &mut self.registry, use_current_dir)?);
        }
        Ok(bundle_trees)
    }
    pub fn get_all<A: AssetGroup>(&self) -> Vec<&A> {
        self.registry.get_all::<A>()
    }
    pub fn get_all_filtered<A: AssetGroup>(
        &self,
        namespace: Option<&str>,
        collection: Option<&str>,
    ) -> Vec<&A> {
        self.registry.get_all_filtered::<A>(namespace, collection)
    }

    pub fn lookup_asset_by_id<A: AssetGroup>(&self, id: u32) -> Option<&A> {
        self.registry.lookup_asset_by_id::<A>(id)
    }

    pub fn lookup_asset<A: AssetGroup>(&self, asset_id: AssetId) -> Option<&A> {
        self.registry.lookup_asset::<A>(asset_id)
    }

    pub fn inner_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    pub fn inner(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    pub fn script(&mut self) -> &mut ScriptRuntime {
        self.registry.script()
    }

    pub fn script_engine(&mut self) -> &mut Engine {
        self.registry.script_engine()
    }

    pub fn call_rhai_fn(
        &self,
        script: &AssetId,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<rhai::Dynamic, String> {
        self.registry.call_rhai_fn(script, function, args)
    }

    pub fn call_fn_ref(&self, function: &RhaiFnRef, args: Vec<Dynamic>) -> Result<Dynamic, String> {
        self.registry.call_fn_ref(function, args)
    }

    pub fn get_rhai_fn(
        &self,
        script: &AssetId,
        function: &str,
    ) -> Result<RhaiFunction<'_>, String> {
        self.registry.get_rhai_fn(script, function)
    }
}
pub type ErasedRaw = Arc<dyn Any>;
pub type RawLoaderFn = fn(&mut Registry, &AssetRef, ron::Value) -> Result<(), String>;
pub(crate) type RawFieldFn = fn(&ErasedRaw, &str) -> Result<ron::Value, String>;

pub type AssetLoaderFn = fn(&mut Registry, AssetRef, &SymbolMap, &ErasedRaw) -> Result<(), String>;

pub struct AssetRegistration {
    pub register_fn: fn(&mut Registry),
}

inventory::collect!(AssetRegistration);

/// Called at engine startup to populate the Registry with all derived asset types
pub fn initialize_registry() -> Registry {
    let mut registry = Registry::default();
    for reg in inventory::iter::<AssetRegistration>() {
        (reg.register_fn)(&mut registry);
    }
    registry
}

#[derive(Default)]
pub struct Registry {
    struct_map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    name_map: HashMap<String, TypeId>,
    raw_loaders: HashMap<String, RawLoaderFn>,
    erased_raws: HashMap<String, ErasedRaw>,
    static_raw_ids: HashSet<String>,
    raw_field_getters: HashMap<TypeId, RawFieldFn>,
    loaders: HashMap<String, AssetLoaderFn>,
    script_run: ScriptRuntime,
}

impl Registry {
    pub fn resolve_asset_id<A: AssetGroup>(
        &self,
        asset_id: &AssetId,
        symbols: &SymbolMap,
        field_name: &str,
    ) -> Result<AssetIdVal<A>, String> {
        if asset_id.field().is_some() {
            return Err(format!(
                "Asset reference '{}' selects a field; Requires AssetFieldVal<Asset,Type>",
                asset_id
            ));
        }
        let asset_id_str = asset_id.get();

        let Some(raw_id) = symbols.get(&asset_id_str) else {
            return Err(format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_id, field_name
            ));
        };

        Ok(AssetIdVal::<A>::from(raw_id.clone()))
    }

    pub fn resolve_asset_field<A: AssetGroup, T: Debug + Copy + DeserializeOwned>(
        &self,
        asset_id: &AssetId,
        symbols: &SymbolMap,
        field_name: &str,
    ) -> Result<AssetFieldVal<A, T>, String> {
        let source_field = asset_id.field().ok_or_else(|| {
            format!(
                "Asset reference '{}' does not select a field for destination field '{}'",
                asset_id, field_name
            )
        })?;
        let asset_id_str = asset_id.get();

        let Some(raw_id) = symbols.get(&asset_id_str) else {
            return Err(format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_id, field_name
            ));
        };

        if !self.static_raw_ids.contains(&asset_id_str) {
            return Err(format!(
                "Projected field reference '{}' for destination field '{}' must target a static asset",
                asset_id, field_name
            ));
        }

        let Some(erased_raw) = self.erased_raws.get(&asset_id_str) else {
            return Err(format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_id, field_name
            ));
        };

        let value = A::get_field::<T>(source_field, erased_raw).map_err(|error| {
            format!(
                "Failed to resolve projected field '{}' for destination field '{}': {}",
                asset_id, field_name, error
            )
        })?;

        Ok(AssetFieldVal::<A, T>::from((raw_id.clone(), value)))
    }

    #[doc(hidden)]
    pub fn __register<A: AssetGroup>(&mut self) {
        let type_id = TypeId::of::<A>();

        if self.name_map.contains_key(&A::STRUCT_NAME.to_string()) {
            eprintln!("Duplicate struct_name skipping {}", A::STRUCT_NAME);
            return;
        }
        let raw_loader = A::load_raw;
        let loader = A::load_ron_asset;
        let raw_field_getter = A::raw_field_value;

        self.struct_map
            .insert(type_id, Box::new(RegistryGlossary::<A>::default()));
        self.name_map.insert(A::STRUCT_NAME.to_string(), type_id);
        self.raw_loaders
            .insert(A::STRUCT_NAME.to_string(), raw_loader);
        self.loaders.insert(A::STRUCT_NAME.to_string(), loader);
        self.raw_field_getters
            .insert(TypeId::of::<A::Raw>(), raw_field_getter);
    }
    #[doc(hidden)]
    pub(crate) fn __add_raw(&mut self, asset_val_string: String, raw: ErasedRaw) {
        self.erased_raws.insert(asset_val_string, raw);
    }

    pub(crate) fn __mark_static_raw(&mut self, asset_id: String) {
        self.static_raw_ids.insert(asset_id);
    }

    pub(crate) fn is_static_raw(&self, asset_id: &str) -> bool {
        self.static_raw_ids.contains(asset_id)
    }

    pub(crate) fn raw_field_getter(&self, type_id: TypeId) -> Option<RawFieldFn> {
        self.raw_field_getters.get(&type_id).copied()
    }

    pub(crate) fn get_erased_raw(&self, asset_val_string: &String) -> Option<&ErasedRaw> {
        self.erased_raws.get(asset_val_string)
    }

    pub fn get<A: AssetGroup>(&self) -> Option<&RegistryGlossary<A>> {
        let type_id = TypeId::of::<A>();
        self.struct_map
            .get(&type_id)
            .and_then(|b| b.downcast_ref::<RegistryGlossary<A>>())
    }
    /// Returns every loaded asset of type `A` in asset ID order.
    pub fn get_all<A: AssetGroup>(&self) -> Vec<&A> {
        self.get()
            .map(RegistryGlossary::get_all)
            .unwrap_or_default()
    }
    /// Returns every loaded glossary item of type `A` in asset ID order.
    pub fn get_all_items<A: AssetGroup>(&self) -> Vec<&GlossaryItem<A>> {
        self.get()
            .map(RegistryGlossary::get_all_items)
            .unwrap_or_default()
    }
    /// Returns assets matching the optional exact namespace/collection filters.
    /// A namespace ending in `.*` matches descendant namespaces, excluding the
    /// namespace before the wildcard.
    pub fn get_all_filtered<A: AssetGroup>(
        &self,
        namespace: Option<&str>,
        collection: Option<&str>,
    ) -> Vec<&A> {
        self.get_all_items_filtered(namespace, collection)
            .into_iter()
            .map(GlossaryItem::get)
            .collect()
    }
    /// Returns glossary items matching the optional exact namespace/collection filters.
    pub fn get_all_items_filtered<A: AssetGroup>(
        &self,
        namespace: Option<&str>,
        collection: Option<&str>,
    ) -> Vec<&GlossaryItem<A>> {
        self.get_all_items::<A>()
            .into_iter()
            .filter(|item| {
                let namespace_matches = item.filter_ns(namespace);
                namespace_matches
                    && collection.is_none_or(|filter| item.get_collection_name() == Some(filter))
            })
            .collect()
    }
    pub fn get_item<A: AssetGroup>(&self, idx: usize) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.get_item(idx)
    }

    pub fn lookup_item<A: AssetGroup>(&self, asset_id: &AssetId) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_item(asset_id)
    }

    pub fn lookup_asset_by_id<A: AssetGroup>(&self, id: u32) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset_by_id(AssetIdVal::<A>::from(id))
    }
    pub fn lookup_id<A: AssetGroup>(&self, asset_id: AssetId) -> Option<AssetIdVal<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_id(&asset_id)
    }
    pub fn lookup_asset<A: AssetGroup>(&self, asset_id: AssetId) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset(&asset_id)
    }

    pub fn lookup_string<A: AssetGroup>(&self, asset_id_str: String) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_string(asset_id_str)
    }

    pub fn get_loader(&self, struct_name: &str) -> Option<&AssetLoaderFn> {
        self.loaders.get(struct_name)
    }

    pub fn get_raw_loader(&self, struct_name: &str) -> Option<&RawLoaderFn> {
        self.raw_loaders.get(struct_name)
    }
    #[doc(hidden)]
    pub fn __get_mut<A: AssetGroup>(&mut self) -> Option<&mut RegistryGlossary<A>> {
        let type_id = TypeId::of::<A>();
        self.struct_map
            .get_mut(&type_id)
            .and_then(|b| b.downcast_mut::<RegistryGlossary<A>>())
    }
    #[doc(hidden)]
    pub fn __load_script(
        &mut self,
        header: AssetHeader,
        symbols: &SymbolMap,
    ) -> Result<(), String> {
        let asset_ref = header.asset_ref.clone();
        let (script, id) = self.script_run.load_script(header, symbols)?;

        self.__get_mut::<RhaiScript>()
            .ok_or_else(|| "Script glossary is not registered".to_owned())?
            .__insert_from_header(script, asset_ref, id)?;
        Ok(())
    }

    pub fn call_rhai_asset_fn<A: AssetGroup>(
        &mut self,
        script: &AssetId,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<&GlossaryItem<A>, String> {
        let item = self
            .lookup_item::<RhaiScript>(&script)
            .ok_or_else(|| format!("Script asset not found {}", script.to_string()))?;
        let dynamic = self.script_run.call(item, function, args)?;
        if dynamic.is::<AssetId>() {
            let asset_id = dynamic.cast::<AssetId>();
            if let Some(item) = self.lookup_item::<A>(&asset_id) {
                Ok(item)
            } else {
                Err(format!(
                    "AssetId for {} does not represent structure {}",
                    function,
                    A::STRUCT_NAME,
                ))
            }
        } else {
            Err(format!(
                "{} did not return Asset Ref, did you mean to use call_rhai_fn?",
                function
            ))
        }
    }

    pub fn call_rhai_fn(
        &self,
        script: &AssetId,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<rhai::Dynamic, String> {
        let item = self
            .lookup_item::<RhaiScript>(&script)
            .ok_or_else(|| format!("Script asset not found {}", script.to_string()))?;
        let dynamic = self.script_run.call(item, function, args)?;
        Ok(dynamic)
    }

    /// Resolve and validate a function reference while converting a static asset.
    pub fn resolve_rhai_fn_ref(
        &self,
        raw: RhaiFnRefRaw,
        owner_str: String,
    ) -> Result<RhaiFnRef, String> {
        self.get_rhai_fn(&raw.asset_ref, &raw.function)?;
        let owner = AssetId::from(owner_str);
        let rhai_ref = RhaiFnRef::new(owner, raw.asset_ref, raw.function);
        Ok(rhai_ref)
    }

    /// Call a function reference stored in a static asset, prepending its owner ID.
    pub fn call_fn_ref(&self, function: &RhaiFnRef, args: Vec<Dynamic>) -> Result<Dynamic, String> {
        let mut all_args = Vec::with_capacity(args.len() + 1);
        all_args.push(Dynamic::from(function.owner().clone()));
        all_args.extend(args);
        self.call_rhai_fn(&function.script(), &function.function(), all_args)
    }

    /// Get a reusable handle for a function in a loaded Rhai script.
    pub fn get_rhai_fn(
        &self,
        script: &AssetId,
        function: &str,
    ) -> Result<RhaiFunction<'_>, String> {
        let item = self
            .lookup_item::<RhaiScript>(script)
            .ok_or_else(|| format!("Script asset not found {}", script))?;
        self.script_run.get_fn(item, function)
    }

    pub fn script_engine(&mut self) -> &mut Engine {
        self.script_run.engine_mut()
    }

    pub fn script(&mut self) -> &mut ScriptRuntime {
        &mut self.script_run
    }

    pub(crate) fn script_view_lookup(&self) -> ViewLookup {
        self.script_run.view_lookup()
    }

    pub fn glossary_items_str<'a, A: AssetGroup>(&self, items: Vec<&GlossaryItem<A>>) -> String {
        let mut full_str = "".to_string();
        for item in items {
            full_str += item.to_string().as_str();
            full_str += " ";
        }

        full_str
    }
}

/// Combines storage and glossary for a single concrete asset type.
#[derive(Debug)]
pub struct RegistryGlossary<A: AssetGroup> {
    items: Vec<Option<GlossaryItem<A>>>,
    id_map: HashMap<AssetId, AssetIdVal<A>>,
    _marker: std::marker::PhantomData<A>,
}

impl<A: AssetGroup> Default for RegistryGlossary<A> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            id_map: HashMap::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<A: AssetGroup> RegistryGlossary<A> {
    #[doc(hidden)]
    pub fn __insert_from_header(
        &mut self,
        asset: A,
        asset_ref: AssetRef,
        val: u32,
    ) -> Result<(), String> {
        let idx = val as usize;

        let asset_val = AssetIdVal::<A>::from(val);
        let asset_id = asset_ref
            .get_id()
            .ok_or_else(|| "Cannot insert an asset without an AssetId".to_owned())?;

        if self.items.len() <= idx {
            self.items.resize_with(idx + 1, || None);
        }

        if self.items[idx].is_some() {
            return Err(format!("Duplicate asset ID insertion for: {:?}", asset_id));
        }

        self.items[idx] = Some(GlossaryItem::<A>::__new(
            asset,
            asset_ref.clone(),
            asset_val,
        ));
        self.id_map.insert(asset_id.clone(), asset_val);

        Ok(())
    }

    pub fn get_item(&self, idx: usize) -> Option<&GlossaryItem<A>> {
        self.items.get(idx).and_then(|opt| opt.as_ref())
    }

    pub fn get_all(&self) -> Vec<&A> {
        self.get_all_items()
            .into_iter()
            .map(GlossaryItem::get)
            .collect()
    }
    pub fn get_all_items(&self) -> Vec<&GlossaryItem<A>> {
        self.items.iter().filter_map(Option::as_ref).collect()
    }

    #[doc(hidden)]
    pub fn __get_mut(&mut self, asset_val: AssetIdVal<A>) -> Option<&mut A> {
        let idx = asset_val.get() as usize;
        let item = self.items[idx].as_mut()?;
        Some(item.__get_mut())
    }

    pub fn lookup_asset_by_id(&self, asset_val: AssetIdVal<A>) -> Option<&A> {
        let idx = asset_val.get() as usize;
        let item: &GlossaryItem<A> = self.items[idx].as_ref()?;
        Some(item.get())
    }

    pub fn lookup_item(&self, asset_id: &AssetId) -> Option<&GlossaryItem<A>> {
        let id = self.lookup_id(asset_id)?;
        let idx = id.get() as usize;
        self.get_item(idx)
    }

    pub fn lookup_id(&self, asset_id: &AssetId) -> Option<AssetIdVal<A>> {
        self.id_map.get(&asset_id).copied()
    }

    pub fn lookup_string(&self, asset_id_str: String) -> Option<&A> {
        let asset_id = &AssetId::from(asset_id_str);
        let id = self.lookup_id(asset_id)?;
        self.lookup_asset_by_id(id)
    }

    pub fn lookup_asset(&self, asset_id: &AssetId) -> Option<&A> {
        let id = self.lookup_id(asset_id)?;
        self.lookup_asset_by_id(id)
    }

    pub fn resolve(&self, id: u32) -> Option<&AssetId> {
        let idx = id as usize;
        if self.items.len() > idx {
            Some(&self.items[idx].as_ref()?.get_id())
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct GlossaryItem<A: AssetGroup> {
    asset: A,
    val: AssetIdVal<A>,
    asset_ref: AssetRef,
    _marker: std::marker::PhantomData<A>,
}
impl<A: AssetGroup> GlossaryItem<A> {
    #[doc(hidden)]
    fn __new(asset: A, asset_ref: AssetRef, val: AssetIdVal<A>) -> Self {
        Self {
            asset,
            val,
            asset_ref,
            _marker: std::marker::PhantomData,
        }
    }
    #[doc(hidden)]
    fn __get_mut(&mut self) -> &mut A {
        &mut self.asset
    }

    pub fn get(&self) -> &A {
        &self.asset
    }

    pub fn get_val(&self) -> AssetIdVal<A> {
        self.val
    }

    pub fn get_ref(&self) -> &AssetRef {
        &self.asset_ref
    }

    fn filter_ns(&self, namespace: Option<&str>) -> bool {
        namespace.is_none_or(|filter| {
            if let Some(prefix) = filter.strip_suffix(".*") {
                self.asset_ref.get_id().is_some_and(|id| {
                    id.namespace
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
                })
            } else {
                self.asset_ref
                    .get_id()
                    .is_some_and(|id| id.namespace.as_ref() == filter)
            }
        })
    }

    pub(crate) fn get_id(&self) -> &AssetId {
        self.asset_ref
            .get_id()
            .expect("loaded glossary items always have an AssetId")
    }
    pub(crate) fn get_collection_name(&self) -> Option<&str> {
        self.asset_ref.get_collection_name()
    }
}

impl<A: AssetGroup> Display for GlossaryItem<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (id: {})", self.get_id(), self.val.get())
    }
}
