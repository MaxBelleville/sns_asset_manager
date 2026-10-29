use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use super::assets;

#[derive(Default)]
pub struct Registry {
    struct_map: HashMap<TypeId, Box<dyn Any>>,
    name_map: HashMap<String, TypeId>, //struct_name,
}

impl Registry {
    pub(crate) fn register<T: assets::AssetGroup>(&mut self, struct_name: String) {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .insert(type_id, Box::new(RegistryGlossary::<T, T::ID>::default()));
        self.name_map.insert(struct_name, type_id);
    }

    pub fn get_from_type(&mut self, struct_name: String) -> Option<&mut Box<dyn Any + 'static>> {
        if let Some(type_id) = self.name_map.get(&struct_name) {
            self.struct_map.get_mut(&type_id)
        } else {
            None
        }
    }

    pub fn get<T: assets::AssetGroup>(&self) -> Option<&RegistryGlossary<T, T::ID>> {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .get(&type_id)
            .and_then(|b| b.downcast_ref::<RegistryGlossary<T, T::ID>>())
    }
    pub(crate) fn get_mut<T: assets::AssetGroup>(
        &mut self,
    ) -> Option<&mut RegistryGlossary<T, T::ID>> {
        let type_id = TypeId::of::<T>();
        self.struct_map
            .get_mut(&type_id)
            .and_then(|b| b.downcast_mut::<RegistryGlossary<T, T::ID>>())
    }
}

/// Combines storage and glossary for a single concrete asset type.
#[derive(Debug)]
pub struct RegistryGlossary<Asset, ID> {
    assets: Vec<Option<Asset>>,
    id_map: HashMap<assets::AssetRef, ID>,
    reverse_id: Vec<assets::AssetRef>,
    _marker: std::marker::PhantomData<ID>,
}
impl<Asset, ID> Default for RegistryGlossary<Asset, ID> {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            id_map: HashMap::new(),
            reverse_id: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Asset, ID: From<u32> + Into<u32> + Copy> RegistryGlossary<Asset, ID> {
    /// Inserts an asset using the pre-calculated ID and AssetRef from Pass 1.5
    pub(crate) fn insert_from_header(
        &mut self,
        asset: Asset,
        asset_ref: assets::AssetRef,
        id_val: u32,
    ) {
        let idx = (id_val as usize).saturating_sub(1);
        let id = ID::from(id_val); // Wrap the raw u32 into the strong ID type
        // Resize storage and reverse tracking vectors if needed
        if self.assets.len() <= idx {
            self.assets.resize_with(idx + 1, || None);
        }
        if self.reverse_id.len() <= idx {
            self.reverse_id.resize(idx + 1, assets::AssetRef::default());
        }

        if self.assets[idx].is_some() {
            panic!("Duplicate asset ID insertion for: {:?}", asset_ref);
        }

        self.assets[idx] = Some(asset);
        self.id_map.insert(asset_ref.clone(), id);
        self.reverse_id[idx] = asset_ref;
    }

    pub fn get(&self, id: ID) -> Option<&Asset> {
        let idx = (id.into() as usize).saturating_sub(1);
        self.assets.get(idx).and_then(|a| a.as_ref())
    }

    pub(crate) fn get_mut(&mut self, id: ID) -> Option<&mut Asset> {
        let idx = (id.into() as usize).saturating_sub(1);
        self.assets.get_mut(idx).and_then(|a| a.as_mut())
    }

    pub fn lookup_id(&self, asset_ref: &assets::AssetRef) -> Option<ID> {
        self.id_map.get(asset_ref).copied()
    }

    pub fn lookup_asset(&self, asset_ref: &assets::AssetRef) -> Option<&Asset> {
        let id = self.lookup_id(asset_ref)?;
        self.get(id)
    }

    pub fn resolve(&self, id: u32) -> &assets::AssetRef {
        let idx = (id as usize).saturating_sub(1);
        &self.reverse_id[idx]
    }
}
