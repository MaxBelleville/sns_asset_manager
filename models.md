# Project model

## Purpose

`sns_asset_manager` is a Rust library for loading user-authored asset bundles. A user supplies bundle metadata plus RON asset files and Rhai script files. The library discovers those files, parses asset headers, resolves references, and loads static asset data into Rust types registered by the consuming application. The `MakeAsset` derive macro generates the conversion and registry-loading code for those Rust types.

## Main concepts

- **Bundle**: A directory (or ZIP archive) with a `.bundle.ron` file. Bundle metadata includes a namespace, version, optional minimum supported version, settings, and include paths. Child bundles inherit applicable metadata and compose namespaces.
- **Asset reference (`AssetRef`)**: A namespaced name such as `core:flags`. Static assets and declarations get their namespace from the containing bundle and their name from `asset_name` or the source filename.
- **Static asset**: User data with `asset_type: Static` and a `struct_name`. The name selects a `MakeAsset`-derived Rust type registered in the `Registry`; the `data` value is deserialized into that type's generated raw form and converted into the final struct.
- **Declaration**: A registered asset type. `Declare(Enum)` defines an ordered list of names and `Declare(Tag)` groups asset references. A static field of type `EnumFlags` can use `Enum(ref: "core:flags", values: ["enable_b"])`; the derive macro resolves it into a typed selection containing the enum declaration ID and a bit mask. A field of type `AssetId<DeclareTag>` can use `Tag("core:some-tag")` to refer to a registered tag. Enum list position determines the bit index (the first item is bit 0); masks support up to 32 declared items.
- **Rhai script**: `.rhai` files are discovered and represented as script headers. Current loading creates script metadata, but the normal static-asset registry loader path does not execute scripts.
- **Asset bundle node**: The returned `AssetBundleNode` tree exposes discovered bundle metadata and child bundles.

## Loading flow

1. The application calls `process_assets(root, &mut registry, use_current_dir)`.
2. The library creates a virtual filesystem for a directory or ZIP, walks bundles/includes, and gathers RON and Rhai headers.
3. Asset references are sorted and assigned per-struct numeric IDs; headers are grouped into declarations, static assets, and scripts.
4. Declaration headers are passed through their registered asset processors before static assets load. Enum declarations validate ordered names; tag declarations validate that their member references exist.
5. Each static header is passed to the processor registered for its `struct_name`. The shared loader deserializes its raw data; `MakeAsset` generated conversion resolves `AssetId<T>` and `EnumFlags` fields, then inserts the result into the registry glossary.
6. The bundle tree is returned to the caller.

## Main code areas

- `src/lib.rs`: Public entry points, filesystem/bundle discovery, RON parsing, ID/symbol assignment, and declaration-first loading.
- `src/assets.rs`: Asset and bundle header types, `AssetType`, `AssetRef`, typed `AssetId<T>`, and returned bundle tree.
- `src/registry.rs`: `Registry`, per-type storage (`RegistryGlossary<T>`), loader registration, and asset lookup.
- `macros/src/lib.rs` and `macros/src/utils.rs`: `#[derive(MakeAsset)]`, which generates raw data structs, field conversion/getters, loaders, and inventory-based registration.
- `src/declares.rs`: Registered `DeclareEnum` and `DeclareTag` runtime types, plus the typed `EnumFlags` selection.
- `examples/assets/`: Sample bundle and asset inputs; `examples/basic.rs` shows a consuming Rust type and registry load.

## Defining Rust asset types

Derive `MakeAsset` for a named-field struct and register it with the inventory mechanism by linking the crate normally. The derived processor uses the struct name in UpperCamelCase by default, or `#[struct_name("...")]` to choose the matching RON `struct_name` explicitly. A field of type `AssetId<T>` is read from RON as an `AssetRef` string and resolved to the referenced asset's typed ID. For `AssetId<DeclareTag>`, use `Tag("namespace:tag")`; `Option<AssetId<DeclareTag>>` and `Vec<AssetId<DeclareTag>>` are also supported. Use `EnumFlags` for RON `Enum(ref: ..., values: [...])` selections; it exposes the declaration ID and selected bits, and can check a named item against its `DeclareEnum` definition. Other fields deserialize directly.

## Input examples

```ron
(
    asset_type: Declare(Enum),
    data: (items: ["enable_a", "enable_b", "enable_c"]),
)
```

```ron
(
    asset_type: Static,
    struct_name: "BasicAll",
    data: (is_loaded: true, flags: Enum(ref: "core:flags", values: ["enable_b"])),
)
```

The enum declaration must be included in the same discovered asset set and use the exact `AssetRef` string in `ref`. Tag declarations use `data: (items: ["core:asset_a", ...])`; each member must exist in the discovered set. Invalid references and unknown enum values are reported as processing errors.

## Useful commands

- `cargo check --all-targets` checks that the library, macros, and examples compile.
- `cargo run --example basic` runs the basic loading example from the repository root.

