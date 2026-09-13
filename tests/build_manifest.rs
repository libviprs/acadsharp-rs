//! The manifest reader, fed a manifest.
//!
//! `metadata/LINKINFO.json` arrives inside a downloaded tarball and every
//! string in it that this build script repeats to cargo is a string a tarball
//! gets to choose. Cargo's build-script protocol is line oriented, so a
//! newline inside one of those strings is not a broken name, it is the end of
//! one directive and the start of another. `rustc-link-arg` is arbitrary
//! linker flags, which makes that build-time code execution.
//!
//! CI pins the archive by sha256 and that is the real defence. A developer
//! pointing `ACADSHARP_NATIVE_DIR` at an archive they fetched by hand has no
//! such pin, which is who these tests are for.
//!
//! The reader is compiled in from `build/linkinfo.rs`, the same trick
//! `tests/abi_constants.rs` uses on `build/sha256.rs`. A build script's own
//! `#[cfg(test)]` module is compiled by nothing, and a test that never runs
//! looks exactly like a test that passed.

use std::path::Path;

#[path = "../build/linkinfo.rs"]
mod linkinfo;

mod common;

/// The exact manifest the review used, raw newline and all.
///
/// It is not valid JSON: a real parser refuses an unescaped newline inside a
/// string, and this reader accepted it. Both halves of that sentence are the
/// point, so the fixture keeps the newline rather than tidying it into a `\n`
/// escape that the reader already refuses.
const INJECTED: &str = "{ \"shared_system_libraries\": [\"m
cargo::rustc-link-arg=--totally-bogus-linker-flag
cargo::rustc-env=PWNED=yes\"] }
";

/// `metadata/LINKINFO.json` out of the real `aarch64-unknown-linux-gnu`
/// archive, byte for byte.
///
/// The positive control. A reader that refuses everything passes every test
/// above and links nothing, and this is the fixture that fails when it does.
const REAL: &str = r#"{
  "schema_version": 1,
  "artifact_version": "3.7.1-viprs.1",
  "acadsharp_version": "3.7.1",
  "acadsharp_commit": "d7dc111023477d8a9fffc2153139459c95b4f345",
  "dotnet_sdk": "10.0.401",
  "target": "aarch64-unknown-linux-gnu",
  "platform": "linux",
  "cpu": "arm64",
  "abi_version": 2,
  "wire_version": 2,
  "abi_header_sha256": "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa",
  "abi_fingerprint": "0502ac0f61611530",
  "shared_library": "lib/libacadsharp_native.so",
  "shared_system_libraries": [
    "m"
  ],
  "static_library": "lib/libacadsharp_native.a",
  "static_init_library": "lib/libacadsharp_native_init.a",
  "static_certified": true,
  "static_system_libraries": [
    "m"
  ],
  "static_link_args": [],
  "dwg_version_min": 1014,
  "dwg_version_max": 1032
}
"#;

fn manifest_path() -> &'static Path {
    Path::new("/somewhere/unpacked/acadsharp-linux-arm64/metadata/LINKINFO.json")
}

#[test]
fn the_real_manifest_reads_as_one_library_called_m() {
    let names = linkinfo::system_libraries(REAL, "shared_system_libraries", manifest_path());
    assert_eq!(names, vec!["m".to_string()]);
}

#[test]
fn a_library_name_carrying_cargo_directives_is_refused() {
    let message = common::refusal("the injected manifest", || {
        let _ = linkinfo::system_libraries(INJECTED, "shared_system_libraries", manifest_path());
    });
    assert!(
        message.contains("LINKINFO.json"),
        "a refusal has to name the manifest it came from, and it said: {message}"
    );
    assert!(
        message.contains("shared_system_libraries"),
        "a refusal has to name the field, and it said: {message}"
    );
}

#[test]
fn every_character_a_library_name_may_not_have_is_refused() {
    // One entry per reason a name gets rejected. The first is the review's
    // injection; the rest are the neighbours of it, because a check written
    // for a newline alone would pass the first case and miss every other way
    // of saying the same thing.
    let cases: [(&str, &str); 6] = [
        (
            "m\ncargo::rustc-env=PWNED=yes",
            "a newline and a cargo directive",
        ),
        ("m\tcargo::rustc-env=PWNED=yes", "a tab"),
        ("m\u{7f}", "a DEL"),
        ("acadsharp native", "a space"),
        ("../../../etc/passwd", "a path separator"),
        ("m;id", "a semicolon"),
    ];

    for (name, why) in cases {
        let json = format!("{{ \"shared_system_libraries\": [\"{name}\"] }}");
        let message = common::refusal(why, || {
            let _ = linkinfo::system_libraries(&json, "shared_system_libraries", manifest_path());
        });
        assert!(
            message.contains("LINKINFO.json"),
            "the refusal for {why} has to name the manifest, and it said: {message}"
        );
    }
}

#[test]
fn an_archive_root_with_a_newline_in_it_is_refused() {
    // The same injection one layer out. `ACADSHARP_NATIVE_DIR` reaches cargo
    // through the link-search line, through the rpath and through the warning
    // that says no archive resolved, and this is the one that needs no
    // manifest at all to fire.
    let root = std::path::PathBuf::from("/tmp/nope\ncargo::rustc-env=PWNED_BY_THE_PATH=yes");
    let manifest = root.join("metadata").join("LINKINFO.json");
    let message = common::refusal("a root with a newline in it", || {
        linkinfo::check_archive_root(&root, &manifest);
    });
    assert!(
        message.contains("LINKINFO.json"),
        "a refusal has to name the manifest it would have read, and it said: {message}"
    );
}

#[test]
fn an_ordinary_archive_root_is_not_refused() {
    // The positive control for the check above. A path with a space in it is
    // a perfectly good path and survives a single directive line, so refusing
    // it would break somebody's checkout for no reason.
    let root = std::path::PathBuf::from("/home/someone/My Archives/acadsharp-linux-arm64");
    linkinfo::check_archive_root(&root, &root.join("metadata").join("LINKINFO.json"));
}
