//! The rebuilt archive's `LINKINFO.json` is committed as the parity lane's
//! provenance (docs/AOT_PARITY.md). This keeps it honest: it has to name the
//! ABI this tree's vendored header carries, or the lane was measured against a
//! library this crate would refuse at the handshake, and the recipe beside it
//! has to name the commit the archive came from.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

#[test]
fn the_rebuilt_lane_was_measured_against_the_abi_this_tree_vendors() {
    let linkinfo = read("tests/data/linkinfo/x86_64-unknown-linux-gnu.3.7.1-viprs.2.json");
    let digest = read("native/viprs_acadsharp.h.sha256");
    let digest = digest.split_whitespace().next().expect("a digest");
    assert!(
        linkinfo.contains(&format!("\"abi_header_sha256\": \"{digest}\"")),
        "the committed rebuilt-lane LINKINFO.json does not carry this tree's header digest {digest}"
    );
    assert!(linkinfo.contains("\"artifact_version\": \"3.7.1-viprs.2\""));
    assert!(linkinfo.contains("\"target\": \"x86_64-unknown-linux-gnu\""));
}

#[test]
fn the_rebuild_recipe_names_the_commit_and_the_report_names_the_recipe() {
    let recipe = read("tests/data/linkinfo/REBUILD.md");
    assert!(recipe.contains("bc351a906d9085f256fb2226fd6050f72e2d3325"));
    assert!(recipe.contains("build_acadsharp.py --platform linux --arch amd64"));
    let report = read("docs/AOT_PARITY.md");
    assert!(report.contains("tests/data/linkinfo/REBUILD.md"));
    assert!(report.contains("bc351a906d9085f256fb2226fd6050f72e2d3325"));
}
