use super::asset_header::AssetHeader;

use super::SymbolMap;
use rhai::{CustomType, Dynamic, Map, TypeBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::asset_shared::{AssetGroup, AssetId, AssetVal, CollectionId};
use super::registry::{AssetRegistration, GlossaryItem, Registry};
use super::scripts::ViewLookup;

/// Raw form of `Tag("namespace:name")` in a static asset field.
#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct TagSelectionRaw(pub AssetId);

/// A tag declaration groups asset references for consumers to query.
#[derive(Debug, Clone)]
pub struct DeclareTag {
    items: HashMap<String, Vec<AssetId>>,
}

impl DeclareTag {
    pub fn items<'a, A: AssetGroup>(
        &self,
        registry: &'a Registry,
    ) -> Result<Vec<&'a GlossaryItem<A>>, String> {
        let vec = self
            .items_for_type(A::STRUCT_NAME)
            .iter()
            .map(|asset_id| {
                registry.lookup_item::<A>(asset_id).ok_or_else(|| {
                    format!(
                        "Tag member {} is not loaded as type '{}'",
                        asset_id,
                        A::STRUCT_NAME
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(vec)
    }

    pub fn map<A: AssetGroup + Clone>(
        &self,
        registry: &Registry,
    ) -> Result<HashMap<AssetVal<A>, A>, String> {
        self.items::<A>(registry).map(|items| {
            items
                .into_iter()
                .map(|item| (item.get_val(), item.get().clone()))
                .collect()
        })
    }

    pub fn contains(&self, asset_id: &AssetId) -> bool {
        self.items.values().any(|items| items.contains(asset_id))
    }

    pub fn items_for_type(&self, struct_name: &str) -> &[AssetId] {
        self.items
            .get(struct_name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

#[doc(hidden)]
#[derive(Debug, Deserialize, Clone)]
pub struct DeclareTagRaw {
    items: HashMap<String, Vec<AssetId>>,
}

#[derive(Debug, Deserialize)]
struct TagSelectorsRaw {
    items: HashMap<String, Vec<String>>,
}

#[derive(Debug)]
enum TagSelector {
    Asset {
        namespace: String,
        name: String,
        wildcard: bool,
    },
    Collection(CollectionId),
}

#[derive(Serialize)]
struct ResolvedTagRaw {
    items: HashMap<String, Vec<String>>,
}

/// Expands tag selectors against discovered headers before declarations load.
/// Expands tag selectors against discovered headers before declarations load.
pub(crate) fn expand_tag_selectors(
    data: ron::Value,
    tag_ref: &AssetId,
    catalog: &[AssetHeader],
) -> Result<ron::Value, String> {
    let raw: TagSelectorsRaw = data
        .into_rust()
        .map_err(|error| format!("Declare(Tag) {}: {}", tag_ref, error))?;

    let mut resolved = HashMap::new();

    for (asset_type, selectors) in raw.items {
        let member_ids = resolve_selectors(&asset_type, selectors, catalog, tag_ref)?;
        resolved.insert(asset_type, member_ids);
    }

    let normalized = ron::to_string(&ResolvedTagRaw { items: resolved })
        .map_err(|error| format!("Failed to normalize Declare(Tag) {}: {}", tag_ref, error))?;

    ron::from_str(&normalized).map_err(|error| {
        format!(
            "Failed to parse normalized Declare(Tag) {}: {}",
            tag_ref, error
        )
    })
}

/// Resolves and deduplicates tag selectors for a specific asset type against the catalog.
fn resolve_selectors(
    asset_type: &str,
    selectors: Vec<String>,
    catalog: &[AssetHeader],
    tag_ref: &AssetId,
) -> Result<Vec<String>, String> {
    let mut member_ids = Vec::new();
    let mut seen = HashSet::new();

    for raw_selector in selectors {
        let selector = parse_tag_selector(&raw_selector)
            .map_err(|error| format!("Declare(Tag) {}: {}", tag_ref, error))?;

        let (matches, warn_if_empty) = match &selector {
            TagSelector::Collection(collection) => {
                let matches: Vec<_> = catalog
                    .iter()
                    .filter(|header| {
                        header.check_struct(asset_type)
                            && header.get_collection().is_some_and(|id| id == collection)
                    })
                    .collect();
                (matches, true)
            }
            TagSelector::Asset {
                namespace,
                name,
                wildcard,
            } => {
                let matches: Vec<_> = catalog
                    .iter()
                    .filter(|header| {
                        let Some(id) = header.get_id() else {
                            return false;
                        };
                        header.check_struct(asset_type)
                            && namespace_matches(&id.namespace, namespace)
                            && (name == "*" || id.name.as_ref() == name)
                    })
                    .collect();

                if matches.is_empty() && !wildcard {
                    return Err(format!(
                        "Declare(Tag) {} references missing asset '{}' for type '{}'",
                        tag_ref, raw_selector, asset_type
                    ));
                }
                (matches, *wildcard)
            }
        };

        if matches.is_empty() && warn_if_empty {
            eprintln!(
                "Warning: Declare(Tag) {} selector '{}' matched no assets of type '{}'",
                tag_ref, raw_selector, asset_type
            );
        }

        for header in matches {
            let asset_id = header
                .get_id()
                .expect("discovered asset headers always have an asset ID")
                .to_string();

            if seen.insert(asset_id.clone()) {
                member_ids.push(asset_id);
            }
        }
    }

    Ok(member_ids)
}

fn parse_tag_selector(selector: &str) -> Result<TagSelector, String> {
    if selector.starts_with('#') {
        return CollectionId::parse_selector(selector).map(TagSelector::Collection);
    }

    let (namespace, name) = selector.split_once(':').ok_or_else(|| {
        format!(
            "Malformed tag selector '{}'; expected #collection or namespace:name",
            selector
        )
    })?;

    if namespace.is_empty() || name.is_empty() || name.contains(':') {
        return Err(format!(
            "Malformed tag selector '{}'; expected namespace:name",
            selector
        ));
    }

    let namespace_wildcard = namespace.ends_with(".*");
    if namespace.contains('*') && !namespace_wildcard {
        return Err(format!(
            "Namespace wildcard in '{}' must be a trailing .*",
            selector
        ));
    }
    if name.contains('*') && name != "*" {
        return Err(format!(
            "Asset-name wildcard in '{}' must be the complete name *",
            selector
        ));
    }
    if namespace.contains('#') {
        return Err(format!(
            "Asset namespaces cannot contain '#': '{}'",
            selector
        ));
    }

    Ok(TagSelector::Asset {
        namespace: namespace.to_owned(),
        name: name.to_owned(),
        wildcard: namespace_wildcard || name == "*",
    })
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

#[derive(Clone)]
pub struct DeclareTagView {
    pub items: Map,
    item_ids: HashMap<String, Vec<AssetId>>,
    view_lookup: ViewLookup,
}

impl DeclareTagView {
    fn from_items(member_ids: &HashMap<String, Vec<AssetId>>, registry: &Registry) -> Self {
        let items = member_ids
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
        Self {
            items,
            item_ids: member_ids.clone(),
            view_lookup: registry.script_view_lookup(),
        }
    }

    fn views(&self) -> Map {
        self.item_ids
            .iter()
            .map(|(asset_type, asset_ids)| {
                let assets = asset_ids
                    .iter()
                    .map(|asset_id| (asset_id.to_string().into(), self.view_lookup.get(asset_id)))
                    .collect::<Map>();
                (asset_type.clone().into(), Dynamic::from(assets))
            })
            .collect()
    }

    #[doc(hidden)]
    pub fn __resolve(raw: &TagSelectionRaw, registry: &Registry) -> Result<Self, String> {
        let item = registry
            .lookup_item::<DeclareTag>(&raw.0)
            .ok_or_else(|| format!("Missing Declare(Tag) asset {}", raw.0))?;
        Ok(Self::from_items(&item.get().items, registry))
    }
}

impl std::fmt::Debug for DeclareTagView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclareTagView")
            .field("items", &self.items)
            .finish_non_exhaustive()
    }
}

impl CustomType for DeclareTagView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("DeclareTag")
            .with_get("items", |view: &mut Self| view.items.clone())
            .with_get("views", |view: &mut Self| view.views());
    }
}

impl AssetGroup for DeclareTag {
    const STRUCT_NAME: &'static str = "DeclareTag";
    type Raw = DeclareTagRaw;
    type View = DeclareTagView;

    fn view_from_raw(raw: &Self::Raw, registry: &Registry) -> Result<Option<Dynamic>, String> {
        Ok(Some(Dynamic::from(DeclareTagView::from_items(
            &raw.items, registry,
        ))))
    }

    fn from_raw(
        raw: Self::Raw,
        _registry: &mut Registry,
        symbols: &SymbolMap,
        _asset_id_string: String,
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

impl AssetVal<DeclareTag> {
    /// Resolves the members of this tag as typed glossary items.
    pub fn items<'a, A: AssetGroup>(
        &self,
        registry: &'a Registry,
    ) -> Result<Vec<&'a GlossaryItem<A>>, String> {
        let tag = registry
            .get_item::<DeclareTag>(self.get() as usize)
            .ok_or_else(|| format!("Declare(Tag) ID {} is not loaded", self.get()))?;

        tag.get().items::<A>(registry)
    }

    /// Clones this tag's members into a map keyed by their typed asset values.
    ///
    /// The returned map is an owned snapshot and does not borrow from the registry.
    pub fn map<A: AssetGroup + Clone>(
        &self,
        registry: &Registry,
    ) -> Result<HashMap<AssetVal<A>, A>, String> {
        let tag = registry
            .get_item::<DeclareTag>(self.get() as usize)
            .ok_or_else(|| format!("Declare(Tag) ID {} is not loaded", self.get()))?;

        tag.get().map::<A>(registry)
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
