use std::{
    any::{Any, TypeId},
    collections::HashMap,
    ops::Sub,
};

use super::{SymbolMap, assets};

pub trait AssetGroup: Sized + 'static {
    const STRUCT_NAME: &'static str;
    fn load_ron_asset(
        registry: &mut Registry,
        asset_ref: assets::AssetRef,
        symbols: &SymbolMap,
        data: ron::Value,
    ) -> Result<(), String>;
}

pub type AssetLoaderFn =
    fn(&mut Registry, assets::AssetRef, &SymbolMap, ron::Value) -> Result<(), String>;

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
    struct_map: HashMap<TypeId, Box<dyn Any>>,
    name_map: HashMap<String, TypeId>,
    reverse_name: Vec<String>,
    loaders: HashMap<String, AssetLoaderFn>,
}

impl Registry {
    pub fn resolve_asset_ref<T: 'static>(
        &self,
        asset_ref: &assets::AssetRef,
        symbols: &SymbolMap,
        field_name: &str,
    ) -> assets::AssetId<T> {
        let asset_ref_str: String = asset_ref.clone().into();
        let raw_id = symbols
            .get(&asset_ref_str)
            .expect(&format!(
                "Failed to resolve asset reference '{}' for field '{}'",
                asset_ref, field_name
            ))
            .clone();

        assets::AssetId::<T>::from(raw_id)
    }

    pub fn register<T: 'static>(&mut self, struct_name: String, loader: AssetLoaderFn) {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .insert(type_id, Box::new(RegistryGlossary::<T>::default()));
        self.name_map.insert(struct_name.clone(), type_id);
        self.reverse_name.push(struct_name.clone());
        self.loaders.insert(struct_name, loader);
    }

    pub fn get_from_type(&mut self, struct_name: String) -> Option<&mut Box<dyn Any + 'static>> {
        if let Some(type_id) = self.name_map.get(&struct_name) {
            self.struct_map.get_mut(&type_id)
        } else {
            None
        }
    }

    pub fn get<T: 'static>(&self) -> Option<&RegistryGlossary<T>> {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .get(&type_id)
            .and_then(|b| b.downcast_ref::<RegistryGlossary<T>>())
    }

    pub fn lookup_asset<T: 'static>(&self, id: u32) -> Option<&T> {
        let glossary = self.get().unwrap();
        glossary.get(assets::AssetId::<T>::from(id))
    }
    pub fn lookup_id<T: 'static>(&self, asset_ref: assets::AssetRef) -> Option<assets::AssetId<T>> {
        let glossary = self.get().unwrap();
        glossary.lookup_id(&asset_ref)
    }

    pub fn lookup_string<T: 'static>(&self, asset_ref_str: String) -> Option<&T> {
        let glossary = self.get().unwrap();
        glossary.lookup_string(asset_ref_str)
    }

    pub fn get_loader(&self, struct_name: &str) -> Option<&AssetLoaderFn> {
        self.loaders.get(struct_name)
    }
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut RegistryGlossary<T>> {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .get_mut(&type_id)
            .and_then(|b| b.downcast_mut::<RegistryGlossary<T>>())
    }
}

/// Combines storage and glossary for a single concrete asset type.
#[derive(Debug)]
pub struct RegistryGlossary<Asset> {
    assets: Vec<Option<Asset>>,
    id_map: HashMap<assets::AssetRef, assets::AssetId<Asset>>,
    reverse_id: Vec<assets::AssetRef>,
    _marker: std::marker::PhantomData<Asset>,
}
impl<Asset> Default for RegistryGlossary<Asset> {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            id_map: HashMap::new(),
            reverse_id: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Asset> RegistryGlossary<Asset> {
    /// Inserts an asset using the pre-calculated ID and AssetRef from Pass 1.5
    pub fn insert_from_header(
        &mut self,
        asset: Asset,
        asset_ref: assets::AssetRef,
        id_val: u32,
    ) -> Result<(), String> {
        let idx = id_val as usize;

        let id = assets::AssetId::<Asset>::from(id_val); // Wrap the raw u32 into the strong ID type
        // Resize storage and reverse tracking vectors if needed
        if self.assets.len() <= idx {
            self.assets.resize_with(idx + 1, || None);
        }
        if self.reverse_id.len() <= idx {
            self.reverse_id.resize(idx + 1, assets::AssetRef::default());
        }

        if self.assets[idx].is_some() {
            return Err(format!("Duplicate asset ID insertion for: {:?}", asset_ref));
        }

        self.assets[idx] = Some(asset);
        self.id_map.insert(asset_ref.clone(), id);
        self.reverse_id[idx] = asset_ref;
        Ok(())
    }

    pub fn get(&self, asset_id: assets::AssetId<Asset>) -> Option<&Asset> {
        let idx = asset_id.get_id() as usize;
        println!("{}", idx);
        self.assets.get(idx).and_then(|a| a.as_ref())
    }

    pub fn get_mut(&mut self, asset_id: assets::AssetId<Asset>) -> Option<&mut Asset> {
        let idx = asset_id.get_id() as usize;
        self.assets.get_mut(idx).and_then(|a| a.as_mut())
    }

    pub fn lookup_id(&self, asset_ref: &assets::AssetRef) -> Option<assets::AssetId<Asset>> {
        self.id_map.get(asset_ref).copied()
    }

    pub fn lookup_string(&self, asset_ref_str: String) -> Option<&Asset> {
        let asset_ref = &assets::AssetRef::from(asset_ref_str);
        let id = self.lookup_id(asset_ref)?;
        self.get(id)
    }

    pub fn lookup_asset(&self, asset_ref: &assets::AssetRef) -> Option<&Asset> {
        let id = self.lookup_id(asset_ref)?;
        self.get(id)
    }

    pub fn resolve(&self, id: u32) -> &assets::AssetRef {
        let idx = id as usize;
        &self.reverse_id[idx]
    }
}
