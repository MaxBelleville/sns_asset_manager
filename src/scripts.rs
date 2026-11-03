use crate::registry::GlossaryItem;

use super::registry::{AssetGroup, AssetRegistration, Registry};
use super::{SymbolMap, assets};
use rhai::{AST, Dynamic, Engine, Scope};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;

/// A script entry stored in the registry. Its typed ID addresses compiled code in `ScriptRuntime`.
#[derive(Debug, Clone)]
pub struct RhaiScript {
    asset_ref: assets::AssetRef,
}

impl RhaiScript {
    pub fn asset_ref(&self) -> &assets::AssetRef {
        &self.asset_ref
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize)]
pub struct RhaiScriptRaw;

impl AssetGroup for RhaiScript {
    const STRUCT_NAME: &'static str = "RhaiScript";
    type Raw = RhaiScriptRaw;

    fn from_raw(
        _raw: Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
    ) -> Result<Self, String> {
        Err("Scripts are compiled and loaded by ScriptRuntime".to_owned())
    }
}

/// Owns the Rhai engine and compiled ASTs for scripts registered in a `Registry`.
#[derive(Default)]
pub struct ScriptRuntime {
    engine: Engine,
    programs: HashMap<u32, AST>,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Access the engine to register host functions and configure Rhai before loading assets.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub(crate) fn load_script(
        &mut self,
        header: assets::AssetHeader,
        symbols: &SymbolMap,
    ) -> Result<(RhaiScript, u32), String> {
        let asset_ref = header.asset_ref.clone();
        let asset_ref_string = asset_ref.to_string();

        let id = *symbols
            .get(&asset_ref_string)
            .ok_or_else(|| format!("No assigned script ID for {asset_ref_string}"))?;

        let mut file = header
            .path
            .open_file()
            .map_err(|error| format!("Could not open script {asset_ref_string}: {error}"))?;

        let mut source = String::new();

        file.read_to_string(&mut source)
            .map_err(|error| format!("Could not read script {asset_ref_string}: {error}"))?;

        let ast = self
            .engine
            .compile(&source)
            .map_err(|error| format!("Failed to compile script {asset_ref_string}: {error}"))?;

        let script = RhaiScript { asset_ref };

        self.programs.insert(id, ast);
        Ok((script, id))
    }

    /// Invoke a named Rhai function from a script registered in `registry`.
    pub fn call(
        &self,
        script_item: &GlossaryItem<RhaiScript>,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<Dynamic, String> {
        let ast = self.programs.get(&script_item.get_id()).ok_or_else(|| {
            format!(
                "Compiled program for script {} is not available",
                script_item.get_ref()
            )
        })?;
        let mut scope = Scope::new();
        self.engine
            .call_fn_with_options::<Dynamic>(
                rhai::CallFnOptions::new(),
                &mut scope,
                ast,
                function,
                args,
            )
            .map_err(|error| {
                format!(
                    "Failed to call '{function}' in script {}: {error}",
                    script_item.get_ref()
                )
            })
    }
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<RhaiScript>();
        }
    }
}
