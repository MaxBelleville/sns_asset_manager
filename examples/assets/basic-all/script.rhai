import "core:helper" as helper;

fn callback(value) {
    let asset = lookup_asset(AssetRef::from("core:asset"));
    let flags = asset.flags;
    let basic_flag = lookup_asset(AssetRef::from("core:asset2"));
    let tag_members = basic_flag.tag.items["BasicEnum"];

    if flags.reference == AssetRef::from("core:flags") && tag_members[0] == AssetRef::from("core:asset") {
        helper::run(value)
    } else {
        0
    }
}
