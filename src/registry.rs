use rhai::{Dynamic, Engine};

use std::fmt::Display;

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use super::{SymbolMap, assets, scripts};

pub type AssetLoaderFn = fn(
    &mut Registry,
    assets::AssetRef,
    Option<String>,
    &SymbolMap,
    ron::Value,
) -> Result<(), String>;

pub struct AssetRegistration {
    pub register_fn: fn(&mut Registry),
}

inventory::collect!(AssetRegistration);

#[cfg(feature = "bevy")]
use super::process_assets;
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
    ) -> Result<Vec<assets::AssetBundleNode>, String> {
        let mut bundle_trees = Vec::new();
        for path in paths {
            let path = Path::new(&path);
            bundle_trees.push(process_assets(&path, &mut self.registry, use_current_dir)?);
        }
        Ok(bundle_trees)
    }
    pub fn get_all<A: assets::AssetGroup>(&self) -> Vec<&A> {
        self.registry.get_all::<A>()
    }
    pub fn get_all_filtered<A: assets::AssetGroup>(
        &self,
        namespace: Option<&str>,
        collection: Option<&str>,
    ) -> Vec<&A> {
        self.registry.get_all_filtered::<A>(namespace, collection)
    }

    pub fn lookup_asset_by_id<A: assets::AssetGroup>(&self, id: u32) -> Option<&A> {
        self.registry.lookup_asset_by_id::<A>(id)
    }

    pub fn lookup_asset<A: assets::AssetGroup>(&self, asset_ref: assets::AssetRef) -> Option<&A> {
        self.registry.lookup_asset::<A>(asset_ref)
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    pub fn script(&mut self) -> &mut scripts::ScriptRuntime {
        self.registry.script()
    }

    pub fn script_engine(&mut self) -> &mut Engine {
        self.registry.script_engine()
    }

    pub fn call_rhai_fn(
        &mut self,
        script: &assets::AssetRef,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<rhai::Dynamic, String> {
        self.registry.call_rhai_fn(script, function, args)
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
    script_run: scripts::ScriptRuntime,
}

impl Registry {
    pub fn resolve_asset_ref<A: assets::AssetGroup>(
        &self,
        asset_ref: &assets::AssetRef,
        symbols: &SymbolMap,
        field_name: &str,
    ) -> assets::AssetId<A> {
        let asset_ref_str: String = asset_ref.to_string();
        let raw_id = symbols
            .get(&asset_ref_str)
            .expect(&format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_ref, field_name
            ))
            .clone();

        assets::AssetId::<A>::from(raw_id)
    }
    #[doc(hidden)]
    pub fn __register<A: assets::AssetGroup>(&mut self) {
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

    pub fn get<A: assets::AssetGroup>(&self) -> Option<&RegistryGlossary<A>> {
        let type_id = TypeId::of::<A>();
        self.struct_map
            .get(&type_id)
            .and_then(|b| b.downcast_ref::<RegistryGlossary<A>>())
    }
    /// Returns every loaded asset of type `A` in asset ID order.
    pub fn get_all<A: assets::AssetGroup>(&self) -> Vec<&A> {
        self.get()
            .map(RegistryGlossary::get_all)
            .unwrap_or_default()
    }
    /// Returns every loaded glossary item of type `A` in asset ID order.
    pub fn get_all_items<A: assets::AssetGroup>(&self) -> Vec<&GlossaryItem<A>> {
        self.get()
            .map(RegistryGlossary::get_all_items)
            .unwrap_or_default()
    }
    /// Returns assets matching the optional exact namespace/collection filters.
    /// A namespace ending in `.*` matches descendant namespaces, excluding the
    /// namespace before the wildcard.
    pub fn get_all_filtered<A: assets::AssetGroup>(
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
    pub fn get_all_items_filtered<A: assets::AssetGroup>(
        &self,
        namespace: Option<&str>,
        collection: Option<&str>,
    ) -> Vec<&GlossaryItem<A>> {
        self.get_all_items::<A>()
            .into_iter()
            .filter(|item| {
                let namespace_matches = item.filter_ns(namespace);
                namespace_matches
                    && collection.is_none_or(|filter| item.collection_name() == Some(filter))
            })
            .collect()
    }
    pub fn get_item<A: assets::AssetGroup>(&self, idx: usize) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.get_item(idx)
    }

    pub fn lookup_item<A: assets::AssetGroup>(
        &self,
        asset_ref: &assets::AssetRef,
    ) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_item(asset_ref)
    }

    pub fn lookup_asset_by_id<A: assets::AssetGroup>(&self, id: u32) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset_by_id(assets::AssetId::<A>::from(id))
    }
    pub fn lookup_id<A: assets::AssetGroup>(
        &self,
        asset_ref: assets::AssetRef,
    ) -> Option<assets::AssetId<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_id(&asset_ref)
    }
    pub fn lookup_asset<A: assets::AssetGroup>(&self, asset_ref: assets::AssetRef) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset(&asset_ref)
    }

    pub fn lookup_string<A: assets::AssetGroup>(&self, asset_ref_str: String) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_string(asset_ref_str)
    }

    pub fn get_loader(&self, struct_name: &str) -> Option<&AssetLoaderFn> {
        self.loaders.get(struct_name)
    }
    #[doc(hidden)]
    pub fn __get_mut<A: assets::AssetGroup>(&mut self) -> Option<&mut RegistryGlossary<A>> {
        let type_id = TypeId::of::<A>();
        self.struct_map
            .get_mut(&type_id)
            .and_then(|b| b.downcast_mut::<RegistryGlossary<A>>())
    }
    #[doc(hidden)]
    pub fn __load_script(
        &mut self,
        header: assets::AssetHeader,
        symbols: &SymbolMap,
    ) -> Result<(), String> {
        let asset_ref = header.asset_ref.clone();
        let collection_name = header.collection_name.clone();
        let (script, id) = self.script_run.load_script(header, symbols)?;

        self.__get_mut::<scripts::RhaiScript>()
            .ok_or_else(|| "Script glossary is not registered".to_owned())?
            .__insert_from_header(script, asset_ref, id, collection_name)?;
        Ok(())
    }

    pub fn call_rhai_asset_fn<A: assets::AssetGroup>(
        &mut self,
        script: &assets::AssetRef,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<&GlossaryItem<A>, String> {
        let item = self
            .lookup_item::<scripts::RhaiScript>(&script)
            .ok_or_else(|| format!("Script asset not found {}", script.to_string()))?;
        let dynamic = self.script_run.call(item, function, args)?;
        if dynamic.is::<assets::AssetRef>() {
            let asset_ref = dynamic.cast::<assets::AssetRef>();
            if let Some(item) = self.lookup_item::<A>(&asset_ref) {
                Ok(item)
            } else {
                Err(format!(
                    "AssetRef for {} does not represent structure {}",
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
        script: &assets::AssetRef,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<rhai::Dynamic, String> {
        let item = self
            .lookup_item::<scripts::RhaiScript>(&script)
            .ok_or_else(|| format!("Script asset not found {}", script.to_string()))?;
        let dynamic = self.script_run.call(item, function, args)?;
        Ok(dynamic)
    }

    pub fn script_engine(&mut self) -> &mut Engine {
        self.script_run.engine_mut()
    }

    pub fn script(&mut self) -> &mut scripts::ScriptRuntime {
        &mut self.script_run
    }
    pub fn glossary_items_str<'a, A: assets::AssetGroup>(
        &self,
        items: Vec<&GlossaryItem<A>>,
    ) -> String {
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
pub struct RegistryGlossary<A: assets::AssetGroup> {
    items: Vec<Option<GlossaryItem<A>>>,
    id_map: HashMap<assets::AssetRef, assets::AssetId<A>>,
    _marker: std::marker::PhantomData<A>,
}

impl<A: assets::AssetGroup> Default for RegistryGlossary<A> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            id_map: HashMap::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<A: assets::AssetGroup> RegistryGlossary<A> {
    #[doc(hidden)]
    pub fn __insert_from_header(
        &mut self,
        asset: A,
        asset_ref: assets::AssetRef,
        id_val: u32,
        collection_name: Option<String>,
    ) -> Result<(), String> {
        let idx = id_val as usize;

        let id = assets::AssetId::<A>::from(id_val);

        if self.items.len() <= idx {
            self.items.resize_with(idx + 1, || None);
        }

        if self.items[idx].is_some() {
            return Err(format!("Duplicate asset ID insertion for: {:?}", asset_ref));
        }

        self.items[idx] = Some(GlossaryItem::<A>::__new(
            asset,
            asset_ref.clone(),
            id,
            collection_name,
        ));
        self.id_map.insert(asset_ref.clone(), id);

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
    pub fn __get_mut(&mut self, asset_id: assets::AssetId<A>) -> Option<&mut A> {
        let idx = asset_id.get() as usize;
        let item = self.items[idx].as_mut()?;
        Some(item.__get_mut())
    }

    pub fn lookup_asset_by_id(&self, asset_id: assets::AssetId<A>) -> Option<&A> {
        let idx = asset_id.get() as usize;
        let item: &GlossaryItem<A> = self.items[idx].as_ref()?;
        Some(item.get())
    }

    pub fn lookup_item(&self, asset_ref: &assets::AssetRef) -> Option<&GlossaryItem<A>> {
        let id = self.lookup_id(asset_ref)?;
        let idx = id.get() as usize;
        self.get_item(idx)
    }

    pub fn lookup_id(&self, asset_ref: &assets::AssetRef) -> Option<assets::AssetId<A>> {
        self.id_map.get(asset_ref).copied()
    }

    pub fn lookup_string(&self, asset_ref_str: String) -> Option<&A> {
        let asset_ref = &assets::AssetRef::from(asset_ref_str);
        let id = self.lookup_id(asset_ref)?;
        self.lookup_asset_by_id(id)
    }

    pub fn lookup_asset(&self, asset_ref: &assets::AssetRef) -> Option<&A> {
        let id = self.lookup_id(asset_ref)?;
        self.lookup_asset_by_id(id)
    }

    pub fn resolve(&self, id: u32) -> Option<&assets::AssetRef> {
        let idx = id as usize;
        if self.items.len() > idx {
            Some(&self.items[idx].as_ref()?.get_ref())
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct GlossaryItem<A: assets::AssetGroup> {
    asset: A,
    id: assets::AssetId<A>,
    asset_ref: assets::AssetRef,
    collection_name: Option<String>,
    _marker: std::marker::PhantomData<A>,
}
impl<A: assets::AssetGroup> GlossaryItem<A> {
    #[doc(hidden)]
    fn __new(
        asset: A,
        asset_ref: assets::AssetRef,
        id: assets::AssetId<A>,
        collection_name: Option<String>,
    ) -> Self {
        Self {
            asset,
            id,
            asset_ref,
            collection_name,
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

    pub fn get_id(&self) -> u32 {
        self.id.get()
    }

    pub fn get_ref(&self) -> &assets::AssetRef {
        &self.asset_ref
    }
    pub fn collection_name(&self) -> Option<&str> {
        self.collection_name.as_deref()
    }

    fn filter_ns(&self, namespace: Option<&str>) -> bool {
        namespace.is_none_or(|filter| {
            if let Some(prefix) = filter.strip_suffix(".*") {
                self.get_ref()
                    .namespace
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
            } else {
                self.get_ref().namespace.as_ref() == filter
            }
        })
    }
}

impl<A: assets::AssetGroup> Display for GlossaryItem<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (id: {})", self.asset_ref, self.id.get())
    }
}
