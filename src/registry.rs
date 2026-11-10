use rhai::{Dynamic, Engine};

use super::SymbolMap;
use super::asset_header::AssetHeader;
use super::asset_shared::{AssetGroup, AssetId, AssetRef, AssetVal};
use super::scripts::{RhaiFunction, RhaiScript, ScriptRuntime};
use std::fmt::Display;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

pub type AssetLoaderFn = fn(&mut Registry, AssetRef, &SymbolMap, ron::Value) -> Result<(), String>;

pub struct AssetRegistration {
    pub register_fn: fn(&mut Registry),
}

inventory::collect!(AssetRegistration);
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

    pub fn registry(&self) -> &Registry {
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
        &mut self,
        script: &AssetId,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<rhai::Dynamic, String> {
        self.registry.call_rhai_fn(script, function, args)
    }

    pub fn get_rhai_fn(
        &self,
        script: &AssetId,
        function: &str,
    ) -> Result<RhaiFunction<'_>, String> {
        self.registry.get_rhai_fn(script, function)
    }
}

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
    loaders: HashMap<String, AssetLoaderFn>,
    script_run: ScriptRuntime,
}

impl Registry {
    pub fn resolve_asset_id<A: AssetGroup>(
        &self,
        asset_id: &AssetId,
        symbols: &SymbolMap,
        field_name: &str,
    ) -> AssetVal<A> {
        let asset_id_str: String = asset_id.to_string();
        let raw_id = symbols
            .get(&asset_id_str)
            .expect(&format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_id, field_name
            ))
            .clone();

        AssetVal::<A>::from(raw_id)
    }
    #[doc(hidden)]
    pub fn __register<A: AssetGroup>(&mut self) {
        let type_id = TypeId::of::<A>();

        if self.name_map.contains_key(&A::STRUCT_NAME.to_string()) {
            eprintln!("Duplicate struct_name skipping {}", A::STRUCT_NAME);
            return;
        }
        let loader = A::load_ron_asset;
        self.struct_map
            .insert(type_id, Box::new(RegistryGlossary::<A>::default()));
        self.name_map.insert(A::STRUCT_NAME.to_string(), type_id);
        self.loaders.insert(A::STRUCT_NAME.to_string(), loader);
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
        glossary.lookup_asset_by_id(AssetVal::<A>::from(id))
    }
    pub fn lookup_id<A: AssetGroup>(&self, asset_id: AssetId) -> Option<AssetVal<A>> {
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
        &mut self,
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
    id_map: HashMap<AssetId, AssetVal<A>>,
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

        let asset_val = AssetVal::<A>::from(val);
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
    pub fn __get_mut(&mut self, asset_val: AssetVal<A>) -> Option<&mut A> {
        let idx = asset_val.get() as usize;
        let item = self.items[idx].as_mut()?;
        Some(item.__get_mut())
    }

    pub fn lookup_asset_by_id(&self, asset_val: AssetVal<A>) -> Option<&A> {
        let idx = asset_val.get() as usize;
        let item: &GlossaryItem<A> = self.items[idx].as_ref()?;
        Some(item.get())
    }

    pub fn lookup_item(&self, asset_id: &AssetId) -> Option<&GlossaryItem<A>> {
        let id = self.lookup_id(asset_id)?;
        let idx = id.get() as usize;
        self.get_item(idx)
    }

    pub fn lookup_id(&self, asset_id: &AssetId) -> Option<AssetVal<A>> {
        self.id_map.get(asset_id).copied()
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
    val: AssetVal<A>,
    asset_ref: AssetRef,
    _marker: std::marker::PhantomData<A>,
}
impl<A: AssetGroup> GlossaryItem<A> {
    #[doc(hidden)]
    fn __new(asset: A, asset_ref: AssetRef, val: AssetVal<A>) -> Self {
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

    pub fn get_val(&self) -> u32 {
        self.val.get()
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
