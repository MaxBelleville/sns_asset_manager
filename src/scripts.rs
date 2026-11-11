use super::SymbolMap;
use super::asset_header::AssetHeader;
use super::asset_shared::{AssetGroup, AssetId, AssetRef};
use super::registry::{AssetRegistration, GlossaryItem, Registry};
use rhai::{AST, Dynamic, Engine, EvalAltResult, Module, ModuleResolver, Position, Scope};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, RwLock};

/// A script entry stored in the registry. Its typed ID addresses compiled code in `ScriptRuntime`.
#[derive(Debug, Clone)]
pub struct RhaiScript {
    asset_id: AssetId,
}

/// An owned reference to a function in a loaded Rhai script.
#[derive(Debug, Clone)]
pub struct RhaiFnRef {
    owner: AssetId,
    script: AssetId,
    function: String,
}

impl RhaiFnRef {
    pub fn script(&self) -> &AssetId {
        &self.script
    }

    pub fn function(&self) -> &str {
        &self.function
    }
    pub fn owner(&self) -> &AssetId {
        &self.owner
    }
    pub(crate) fn new(owner: AssetId, script: AssetId, function: String) -> Self {
        Self {
            script,
            function,
            owner,
        }
    }
}

/// Raw RON representation used for a `RhaiFnRef` static asset field.
#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct RhaiFnRefRaw {
    pub asset_ref: AssetId,
    #[serde(rename = "fn")]
    pub function: String,
}

/// A reusable handle to a function in a loaded Rhai script.
///
/// The handle borrows the runtime's engine and compiled script, so it cannot
/// outlive the runtime or be used while the registry is mutably borrowed.
pub struct RhaiFunction<'a> {
    engine: &'a Engine,
    ast: &'a AST,
    script_id: AssetId,
    function: String,
}

impl RhaiFunction<'_> {
    /// Call the script function with Rhai dynamic arguments.
    pub fn call(&self, args: Vec<Dynamic>) -> Result<Dynamic, String> {
        let mut scope = Scope::new();
        self.engine
            .call_fn_with_options::<Dynamic>(
                rhai::CallFnOptions::new(),
                &mut scope,
                self.ast,
                &self.function,
                args,
            )
            .map_err(|error| {
                format!(
                    "Failed to call '{}' in script {}: {error}",
                    self.function, self.script_id
                )
            })
    }
}

impl RhaiScript {
    pub fn asset_id(&self) -> &AssetId {
        &self.asset_id
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct RhaiScriptRaw;

impl AssetGroup for RhaiScript {
    const STRUCT_NAME: &'static str = "RhaiScript";
    type Raw = RhaiScriptRaw;
    type View = ();

    fn from_raw(
        _raw: Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
        _asset_id_string: String,
    ) -> Result<Self, String> {
        Err("Scripts are compiled and loaded by ScriptRuntime".to_owned())
    }
}

type ViewStore = Arc<RwLock<HashMap<AssetId, StoredView>>>;
type ProgramStore = Arc<RwLock<HashMap<AssetId, AST>>>;

#[derive(Clone)]
struct StoredView {
    value: Dynamic,
    collection_name: Option<String>,
}

fn optional_filter(value: Dynamic, parameter: &str) -> Result<Option<String>, Box<EvalAltResult>> {
    if value.is_unit() {
        return Ok(None);
    }
    if value.is::<rhai::ImmutableString>() {
        return Ok(Some(value.cast::<rhai::ImmutableString>().to_string()));
    }
    Err(Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(format!(
            "lookup_filtered {parameter} must be a string or ()"
        )),
        Position::NONE,
    )))
}

fn namespace_matches(namespace: &str, filter: &str) -> bool {
    if let Some(prefix) = filter.strip_suffix(".*") {
        namespace
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
    } else {
        namespace == filter
    }
}

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
        let views = Arc::new(RwLock::new(HashMap::<AssetId, StoredView>::new()));
        let program_refs = Arc::new(RwLock::new(HashMap::<AssetId, AST>::new()));
        let mut engine = Engine::new();

        engine.build_type::<AssetId>();
        engine.register_fn("==", |left: &mut AssetId, right: AssetId| *left == right);
        engine.register_fn("!=", |left: &mut AssetId, right: AssetId| *left != right);

        let mut asset_id_module = Module::new();

        asset_id_module.set_native_fn(
            "from",
            |value: rhai::ImmutableString| -> Result<AssetId, Box<EvalAltResult>> {
                AssetId::parse(value.as_str()).map_err(|error| {
                    Box::new(EvalAltResult::ErrorRuntime(
                        Dynamic::from(error),
                        Position::NONE,
                    ))
                })
            },
        );

        engine.register_static_module("AssetId", asset_id_module.into());

        let view_lookup = Arc::clone(&views);
        engine.register_fn("lookup_asset", move |asset_id: AssetId| {
            view_lookup
                .read()
                .ok()
                .and_then(|assets| assets.get(&asset_id).map(|view| view.value.clone()))
                .unwrap_or(Dynamic::UNIT)
        });

        let filtered_view_lookup = Arc::clone(&views);
        engine.register_fn(
            "lookup_filtered",
            move |namespace: Dynamic,
                  collection: Dynamic|
                  -> Result<rhai::Array, Box<EvalAltResult>> {
                let namespace = optional_filter(namespace, "namespace")?;
                let collection = optional_filter(collection, "collection")?;
                let views = filtered_view_lookup.read().map_err(|_| {
                    Box::new(EvalAltResult::ErrorRuntime(
                        Dynamic::from("Asset view store is unavailable"),
                        Position::NONE,
                    ))
                })?;
                let mut matching = views
                    .iter()
                    .filter(|(asset_id, view)| {
                        namespace
                            .as_deref()
                            .is_none_or(|filter| namespace_matches(&asset_id.namespace, filter))
                            && collection.as_deref().is_none_or(|filter| {
                                view.collection_name.as_deref() == Some(filter)
                            })
                    })
                    .map(|(asset_id, view)| (asset_id.to_string(), view.value.clone()))
                    .collect::<Vec<_>>();
                matching.sort_by(|left, right| left.0.cmp(&right.0));
                Ok(matching.into_iter().map(|(_, value)| value).collect())
            },
        );

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

    pub(crate) fn insert_view(&mut self, asset_ref: AssetRef, view: Dynamic) -> Result<(), String> {
        let asset_id = asset_ref
            .get_id()
            .ok_or_else(|| "Cannot register a view without an AssetId".to_owned())?
            .clone();
        let mut views = self
            .views
            .write()
            .map_err(|_| "Asset view store is unavailable".to_owned())?;
        views.insert(
            asset_id,
            StoredView {
                value: view,
                collection_name: asset_ref.get_collection_name().map(str::to_owned),
            },
        );
        Ok(())
    }

    pub(crate) fn load_script(
        &mut self,
        header: AssetHeader,
        symbols: &SymbolMap,
    ) -> Result<(RhaiScript, u32), String> {
        let asset_id = header
            .get_id()
            .ok_or_else(|| "Script header is missing its AssetId".to_owned())?
            .clone();
        let asset_id_string = asset_id.to_string();

        let id = *symbols
            .get(&asset_id_string)
            .ok_or_else(|| format!("No assigned script ID for {asset_id_string}"))?;

        let mut file = header
            .path
            .open_file()
            .map_err(|error| format!("Could not open script {asset_id_string}: {error}"))?;
        let mut source = String::new();

        file.read_to_string(&mut source)
            .map_err(|error| format!("Could not read script {asset_id_string}: {error}"))?;

        let mut ast = self
            .engine
            .compile(&source)
            .map_err(|error| format!("Failed to compile script {asset_id_string}: {error}"))?;
        ast.set_source(asset_id_string.clone());

        if let Ok(mut programs) = self.program_refs.write() {
            programs.insert(asset_id.clone(), ast.clone());
        }
        self.programs.insert(id, ast);

        Ok((RhaiScript { asset_id }, id))
    }

    /// Invoke a named Rhai function from a script registered in `registry`.
    pub fn call(
        &self,
        script_item: &GlossaryItem<RhaiScript>,
        function: &str,
        args: Vec<Dynamic>,
    ) -> Result<Dynamic, String> {
        self.get_fn(script_item, function)?.call(args)
    }

    /// Create a reusable handle for a named function in a loaded script.
    pub(crate) fn get_fn<'a>(
        &'a self,
        script_item: &GlossaryItem<RhaiScript>,
        function: &str,
    ) -> Result<RhaiFunction<'a>, String> {
        let ast = self.programs.get(&script_item.get_val()).ok_or_else(|| {
            format!(
                "Compiled program for script {} is not available",
                script_item.get_id()
            )
        })?;
        if !ast
            .iter_functions()
            .any(|metadata| metadata.name == function && metadata.access.is_public())
        {
            return Err(format!(
                "Public function '{}' was not found in script {}",
                function,
                script_item.get_id()
            ));
        }
        Ok(RhaiFunction {
            engine: &self.engine,
            ast,
            script_id: script_item.get_id().clone(),
            function: function.to_owned(),
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
        let asset_id = AssetId::from(path.to_owned());
        let ast = self
            .programs
            .read()
            .ok()
            .and_then(|programs| programs.get(&asset_id).cloned())
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
        let asset_id = AssetId::from(path.to_owned());
        self.programs
            .read()
            .ok()
            .and_then(|programs| programs.get(&asset_id).cloned())
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
