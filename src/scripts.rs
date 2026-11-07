use crate::registry::{AssetRegistration, GlossaryItem, Registry};
use crate::{SymbolMap, assets};
use rhai::{AST, Dynamic, Engine, EvalAltResult, Module, ModuleResolver, Position, Scope};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, RwLock};

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
#[derive(Debug, Deserialize, Clone)]
pub struct RhaiScriptRaw;

impl assets::AssetGroup for RhaiScript {
    const STRUCT_NAME: &'static str = "RhaiScript";
    type Raw = RhaiScriptRaw;
    type View = ();

    fn from_raw(
        _raw: Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
    ) -> Result<Self, String> {
        Err("Scripts are compiled and loaded by ScriptRuntime".to_owned())
    }
}

type ViewStore = Arc<RwLock<HashMap<assets::AssetRef, Dynamic>>>;
type ProgramStore = Arc<RwLock<HashMap<assets::AssetRef, AST>>>;

/// Owns the Rhai engine, compiled programs, and read-only asset views.
pub struct ScriptRuntime {
    engine: Engine,
    programs: HashMap<u32, AST>,
    program_refs: ProgramStore,
    views: ViewStore,
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptRuntime {
    pub fn new() -> Self {
        let views = Arc::new(RwLock::new(HashMap::<assets::AssetRef, Dynamic>::new()));
        let program_refs = Arc::new(RwLock::new(HashMap::<assets::AssetRef, AST>::new()));
        let mut engine = Engine::new();

        engine.build_type::<assets::AssetRef>();
        engine.register_fn(
            "==",
            |left: &mut assets::AssetRef, right: assets::AssetRef| *left == right,
        );
        engine.register_fn(
            "!=",
            |left: &mut assets::AssetRef, right: assets::AssetRef| *left != right,
        );

        let mut asset_ref_module = Module::new();

        asset_ref_module.set_native_fn("from", |value: rhai::ImmutableString| {
            Ok::<_, Box<EvalAltResult>>(assets::AssetRef::from(value.to_string()))
        });

        engine.register_static_module("AssetRef", asset_ref_module.into());

        let view_lookup = Arc::clone(&views);
        engine.register_fn("lookup_asset", move |asset_ref: assets::AssetRef| {
            view_lookup
                .read()
                .ok()
                .and_then(|assets| assets.get(&asset_ref).cloned())
                .unwrap_or(Dynamic::UNIT)
        });

        engine.register_fn("is_unit", |value: &mut Dynamic| value.is_unit());

        engine.set_module_resolver(AssetModuleResolver {
            programs: Arc::clone(&program_refs),
        });

        Self {
            engine,
            programs: HashMap::new(),
            program_refs,
            views,
        }
    }

    /// Access the engine to register host functions and configure Rhai before loading assets.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub(crate) fn insert_view(&mut self, asset_ref: assets::AssetRef, view: Dynamic) {
        if let Ok(mut views) = self.views.write() {
            views.insert(asset_ref, view);
        }
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

        let mut ast = self
            .engine
            .compile(&source)
            .map_err(|error| format!("Failed to compile script {asset_ref_string}: {error}"))?;
        ast.set_source(asset_ref_string.clone());

        if let Ok(mut programs) = self.program_refs.write() {
            programs.insert(asset_ref.clone(), ast.clone());
        }
        self.programs.insert(id, ast);

        Ok((RhaiScript { asset_ref }, id))
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

struct AssetModuleResolver {
    programs: ProgramStore,
}

impl ModuleResolver for AssetModuleResolver {
    fn resolve(
        &self,
        engine: &Engine,
        _source: Option<&str>,
        path: &str,
        position: Position,
    ) -> Result<rhai::Shared<Module>, Box<EvalAltResult>> {
        let asset_ref = assets::AssetRef::from(path.to_owned());
        let ast = self
            .programs
            .read()
            .ok()
            .and_then(|programs| programs.get(&asset_ref).cloned())
            .ok_or_else(|| {
                Box::new(EvalAltResult::ErrorModuleNotFound(
                    path.to_owned(),
                    position,
                ))
            })?;
        let mut module = Module::eval_ast_as_new(Scope::new(), &ast, engine).map_err(|error| {
            Box::new(EvalAltResult::ErrorInModule(
                path.to_owned(),
                error,
                position,
            ))
        })?;
        module.build_index();
        Ok(module.into())
    }

    fn resolve_ast(
        &self,
        _engine: &Engine,
        _source: Option<&str>,
        path: &str,
        _position: Position,
    ) -> Option<Result<AST, Box<EvalAltResult>>> {
        let asset_ref = assets::AssetRef::from(path.to_owned());
        self.programs
            .read()
            .ok()
            .and_then(|programs| programs.get(&asset_ref).cloned())
            .map(Ok)
    }
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| {
            registry.__register::<RhaiScript>();
        }
    }
}
