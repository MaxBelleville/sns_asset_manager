# Project model

## Purpose

`sns_asset_manager` is a Rust library for loading user-authored asset bundles. A user supplies bundle metadata plus RON asset files and Rhai script files. The library discovers those files, parses asset headers, resolves references, and loads static asset data into Rust types registered by the consuming application. The `MakeAsset` derive macro generates the conversion and registry-loading code for those Rust types.

## Main concepts

- **Bundle**: A directory (or ZIP archive) with a `.bundle.ron` file. Bundle metadata includes a namespace, version, optional minimum supported version, settings, and include paths. Child bundles inherit applicable metadata and compose namespaces.
- **Asset reference (`AssetId`)**: A namespaced name such as `core:flags`. Static assets and declarations get their namespace from the containing bundle and their name from `asset_name` or the source filename.
- **Static asset**: User data with `asset_type: Static` and a `struct_name`. The name selects a `MakeAsset`-derived Rust type registered in the `Registry`; the `data` value is deserialized into that type's generated raw form and converted into the final struct.
- **Declaration**: A registered asset type. `Declare(Enum)` defines an ordered list of names and `Declare(Tag)` groups references by registered `STRUCT_NAME`. A static field of type `EnumFlags` can use `Enum(ref: "core:flags", values: ["enable_b"])`; the derive macro resolves it into a typed selection containing the enum declaration ID and a bit mask. A field of type `AssetVal<DeclareTag>` can use `Tag("core:some-tag")`; calling `tag.items::<MyAsset>(&registry)` returns matching typed glossary items directly. Enum list position determines the bit index (the first item is bit 0); masks support up to 32 declared items.
- **Rhai view**: `MakeAsset` generates an immutable view from decoded RON raw values. Fields are included by default; `#[exclude_from_rhai]` hides a field, and the same struct attribute omits the entire view. Views are indexed by `AssetId` and retrieved in scripts with `lookup_asset(AssetId::from("namespace:name"))`.
- **Rhai script**: `.rhai` files are discovered and compiled after declarations and static assets load. Scripts can import other scripts by asset reference (for example, `import "core:helpers" as helpers;`). The registry-owned runtime supports Rust callbacks and named function calls.
- **Asset bundle node**: The returned `AssetBundleNode` tree exposes discovered bundle metadata and child bundles.

## Loading flow

1. The application calls `process_assets(root, &mut registry, use_current_dir)`; the registry owns the script runtime. Register host functions through `registry.script_engine()` before loading.
2. The library creates a virtual filesystem for a directory or ZIP, walks bundles/includes, and gathers RON and Rhai headers.
3. Asset references are sorted and assigned per-struct numeric IDs; headers are grouped into declarations, static assets, and scripts.
4. Declaration headers are passed through their registered asset processors before static assets load. Enum declarations validate ordered names; tag declarations validate that every member exists, is unique, and appears under its actual registered type name.
5. Each asset loader deserializes a raw value. It projects any Rhai view from that raw value, converts the value into its registered Rust type, inserts the asset, and attaches its view under the same `AssetId`.
6. Enum selection views retain the declaration reference and selected names. Tag reference views expand the declaration into a map of asset type names to member references.
7. Rhai sources compile through the configured runtime; the module resolver loads imported scripts from their registered asset references. `lookup_asset` returns Rhai unit when an asset is missing or excluded.

## Main code areas

- `src/lib.rs`: Public entry points, filesystem/bundle discovery, RON parsing, ID/symbol assignment, and declaration-first loading.
- `src/assets.rs`: Asset and bundle header types, `AssetType`, `AssetId`, typed `AssetVal<T>`, and returned bundle tree.
- `src/registry.rs`: `Registry`, per-type storage (`RegistryGlossary<T>`), loader registration, and asset lookup.
- `macros/src/lib.rs` and `macros/src/utils.rs`: `#[derive(MakeAsset)]`, which generates raw data structs, field conversion/getters, loaders, and inventory-based registration.
- `src/declares.rs`: Registered `DeclareEnum` and `DeclareTag` runtime types, plus the typed `EnumFlags` selection.
- `src/scripts.rs`: Registry-backed script handles and the Rhai engine/program runtime.
- `examples/assets/`: Sample bundle and asset inputs; `examples/basic.rs` shows a consuming Rust type and registry load.

## Defining Rust asset types

Derive `MakeAsset` for a named-field struct and register it with inventory by linking the crate normally. The derived processor uses the struct name in UpperCamelCase by default, or `#[struct_name("...")]` to choose the matching RON `struct_name`. Fields appear in the Rhai view by default; mark an unsupported or private field with `#[exclude_from_rhai]`. Apply that attribute to the struct to omit its view. Exposed fields must be cloneable, thread-safe, and representable in Rhai. `Vec` fields become Rhai arrays. `AssetVal<T>` fields use `AssetId` references; `Tag("namespace:tag")` fields expose the referenced tag's grouped members, while `Enum(ref: ..., values: [...])` fields expose the declaration reference and selected values.

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

```ron
(
    asset_type: Declare(Tag),
    data: (items: {
        "BasicEnum": ["core:asset"],
        "DeclareEnum": ["core:flags"],
    }),
)
```

The enum declaration must be included in the same discovered asset set and use the exact `AssetId` string in `ref`. Tag map keys must match each member's registered `STRUCT_NAME`; each reference must exist in the discovered set. Invalid references, type mismatches, and unknown enum values are reported as processing errors.

## Useful commands

- `cargo check --all-targets` checks that the library, macros, and examples compile.
- `cargo run --example basic` runs the basic loading example from the repository root.
