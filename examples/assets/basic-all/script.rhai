import "core:helper" as helper;

fn callback(value) {
    let asset = lookup_asset(AssetId::from("core:asset"));
    let flags = asset.flags;
    let basic_flag = lookup_asset(AssetId::from("core:asset2"));
    let tag_members = basic_flag.tag.items["BasicEnum"];

    if flags.reference == AssetId::from("core:flags") && tag_members[0] == AssetId::from("core:asset") {
        helper::run(value)
    } else {
        0
    }
}
