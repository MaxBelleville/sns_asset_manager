use rhai::{Dynamic, Engine};
use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::assets::AssetRef;
use crate::scripts::RhaiScript;

use super::scripts;

use super::{SymbolMap, assets};

pub type AssetLoaderFn =
    fn(&mut Registry, assets::AssetRef, &SymbolMap, ron::Value) -> Result<(), String>;

pub struct AssetRegistration {
    pub register_fn: fn(&mut Registry),
}

inventory::collect!(AssetRegistration);
pub trait AssetGroup: Sized + 'static {
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
        asset_ref: assets::AssetRef,
        symbols: &SymbolMap,
        data: ron::Value,
    ) -> Result<(), String> {
        let asset_ref_string = asset_ref.to_string();
        let raw: Self::Raw = data
            .into_rust()
            .map_err(|error| format!("{} - {}: {}", Self::STRUCT_NAME, asset_ref_string, error))?;

        let id = symbols.get(&asset_ref_string).ok_or_else(|| {
            format!(
                "No assigned asset ID for {} ({})",
                Self::STRUCT_NAME,
                asset_ref_string
            )
        })?;

        let view = Self::view_from_raw(&raw, registry)?;

        let asset = Self::from_raw(raw, registry, symbols)?;
        let view_ref = asset_ref.clone();

        let glossary = registry
            .get_mut::<Self>()
            .ok_or_else(|| format!("Glossary not found for type: {}", Self::STRUCT_NAME))?;

        glossary.__insert_from_header(asset, asset_ref, *id)?;
        if let Some(view) = view {
            registry.script_run.insert_view(view_ref, view);
        }
        Ok(())
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
    struct_map: HashMap<TypeId, Box<dyn Any>>,
    name_map: HashMap<String, TypeId>,
    loaders: HashMap<String, AssetLoaderFn>,
    script_run: scripts::ScriptRuntime,
}

impl Registry {
    pub fn resolve_asset_ref<A: AssetGroup>(
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
    pub fn get_item<A: AssetGroup>(&self, idx: usize) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.get_item(idx)
    }

    pub fn lookup_item<A: AssetGroup>(
        &self,
        asset_ref: &assets::AssetRef,
    ) -> Option<&GlossaryItem<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_item(asset_ref)
    }

    pub fn lookup_asset_by_id<A: AssetGroup>(&self, id: u32) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset_by_id(assets::AssetId::<A>::from(id))
    }
    pub fn lookup_id<A: AssetGroup>(
        &self,
        asset_ref: assets::AssetRef,
    ) -> Option<assets::AssetId<A>> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_id(&asset_ref)
    }
    pub fn lookup_asset<A: AssetGroup>(&self, asset_ref: AssetRef) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_asset(&asset_ref)
    }

    pub fn lookup_string<A: AssetGroup>(&self, asset_ref_str: String) -> Option<&A> {
        let Some(glossary) = self.get() else {
            return None;
        };
        glossary.lookup_string(asset_ref_str)
    }

    pub fn get_loader(&self, struct_name: &str) -> Option<&AssetLoaderFn> {
        self.loaders.get(struct_name)
    }
    pub fn get_mut<A: AssetGroup>(&mut self) -> Option<&mut RegistryGlossary<A>> {
        let type_id = TypeId::of::<A>();
        self.struct_map
            .get_mut(&type_id)
            .and_then(|b| b.downcast_mut::<RegistryGlossary<A>>())
    }

    pub fn load_script(
        &mut self,
        header: assets::AssetHeader,
        symbols: &SymbolMap,
    ) -> Result<(), String> {
        let asset_ref = header.asset_ref.clone();
        let (script, id) = self.script_run.load_script(header, symbols)?;

        self.get_mut::<scripts::RhaiScript>()
            .ok_or_else(|| "Script glossary is not registered".to_owned())?
            .__insert_from_header(script, asset_ref, id)?;
        Ok(())
    }

    pub fn call_rhai_asset_fn<A: AssetGroup>(
        &mut self,
        script: &assets::AssetRef,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<&GlossaryItem<A>, String> {
        let item = self
            .lookup_item::<RhaiScript>(&script)
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
            .lookup_item::<RhaiScript>(&script)
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
    id_map: HashMap<assets::AssetRef, assets::AssetId<A>>,
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
        asset_ref: assets::AssetRef,
        id_val: u32,
    ) -> Result<(), String> {
        let idx = id_val as usize;

        let id = assets::AssetId::<A>::from(id_val);

        if self.items.len() <= idx {
            self.items.resize_with(idx + 1, || None);
        }

        if self.items[idx].is_some() {
            return Err(format!("Duplicate asset ID insertion for: {:?}", asset_ref));
        }

        self.items[idx] = Some(GlossaryItem::<A>::__new(asset, asset_ref.clone(), id));
        self.id_map.insert(asset_ref.clone(), id);

        Ok(())
    }

    pub fn get_item(&self, idx: usize) -> Option<&GlossaryItem<A>> {
        self.items.get(idx).and_then(|opt| opt.as_ref())
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
pub struct GlossaryItem<A: AssetGroup> {
    asset: A,
    id: assets::AssetId<A>,
    asset_ref: assets::AssetRef,
    _marker: std::marker::PhantomData<A>,
}
impl<A: AssetGroup> GlossaryItem<A> {
    #[doc(hidden)]
    fn __new(asset: A, asset_ref: assets::AssetRef, id: assets::AssetId<A>) -> Self {
        Self {
            asset,
            id,
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

    pub fn get_id(&self) -> u32 {
        self.id.get()
    }

    pub fn get_ref(&self) -> &assets::AssetRef {
        &self.asset_ref
    }
}

impl<A: AssetGroup> Display for GlossaryItem<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (id: {})", self.asset_ref, self.id.get())
    }
}
