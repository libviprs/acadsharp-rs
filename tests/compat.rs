//! `COMPAT.toml`, the declaration of what this crate is compatible with, and
//! every check that makes it load bearing.
//!
//! The file says five things: the ABI version, the wire version, the digest of
//! the vendored header, a glob over the native artifact version, and the list
//! of upstream ACadSharp versions. None of those five is a number this crate
//! gets to choose. Two of them are already derived from the header's bytes by
//! `build.rs`, one is that header's digest, and the last two describe archives
//! somebody else publishes. So every test in here compares the declaration
//! against the thing it declares, never against a second copy of itself.
//!
//! The parser is compiled in from `build/compat.rs`, the same trick
//! `tests/abi_constants.rs` uses on `build/sha256.rs` and
//! `tests/build_manifest.rs` uses on `build/linkinfo.rs`. A build script's own
//! `#[cfg(test)]` module is compiled by nothing, and a test that never runs
//! looks exactly like a test that passed.

use std::path::{Path, PathBuf};

use acadsharp_rs::{EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};

#[path = "../build/compat.rs"]
mod compat;

#[allow(dead_code)]
#[path = "../build/sha256.rs"]
mod sha256;

mod common;

/// The repository root, resolved from the manifest directory so none of this
/// cares what the working directory is.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn compat_path() -> PathBuf {
    repo_root().join(compat::COMPAT_FILE)
}

fn declaration() -> compat::Compat {
    compat::Compat::read(&compat_path())
}

/// `metadata/LINKINFO.json` out of the real `aarch64-unknown-linux-gnu`
/// archive, byte for byte.
///
/// The positive control for the archive checks, and it is a copy of the real
/// file rather than something I wrote, because a fixture written by the same
/// hand as the check it feeds proves only that the hand is consistent. The
/// archive-free CI jobs have no archive to read, so this is what they check
/// against; the jobs that do have one check against that as well, below.
const REAL_MANIFEST: &str = r#"{
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

/// A manifest path to put in a refusal message when the manifest came from a
/// string rather than from a file.
fn fake_manifest_path() -> PathBuf {
    PathBuf::from("the manifest fixture in tests/compat.rs")
}

// ---------------------------------------------------------------------------
// The file itself
// ---------------------------------------------------------------------------

#[test]
fn compat_toml_exists_at_the_repo_root() {
    let path = compat_path();
    assert!(
        path.is_file(),
        "{} is missing. It is the compatibility declaration and the build reads it, so its \
         absence is not a degraded build, it is no build at all.",
        path.display()
    );
}

#[test]
fn compat_toml_holds_exactly_the_five_keys() {
    let declared = declaration();
    // Reading every field, so adding a sixth key without a check here is a
    // compile error over there rather than a decoration nobody notices.
    let compat::Compat {
        abi_version,
        wire_version,
        abi_header_sha256,
        native_artifact_versions,
        acadsharp_versions,
    } = &declared;
    assert!(*abi_version > 0, "`abi_version` has to be a real version");
    assert!(*wire_version > 0, "`wire_version` has to be a real version");
    assert_eq!(
        abi_header_sha256.len(),
        64,
        "`abi_header_sha256` is a sha256, so it is 64 hex characters and not {abi_header_sha256:?}"
    );
    assert!(
        abi_header_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "`abi_header_sha256` has to be lowercase hex with no `0x`, and it is {abi_header_sha256:?}"
    );
    assert!(
        !native_artifact_versions.is_empty(),
        "`native_artifact_versions` is the glob every archive is matched against, so an empty one \
         matches nothing and stops every build that has an archive"
    );
    assert!(
        !acadsharp_versions.is_empty(),
        "`acadsharp_versions` with nothing in it refuses every archive there is"
    );
}

#[test]
fn a_key_the_declaration_does_not_know_is_refused() {
    let text = format!(
        "{}\nsomething_new = 3\n",
        std::fs::read_to_string(compat_path()).expect("COMPAT.toml is missing")
    );
    let message = common::refusal("an unknown key in COMPAT.toml", || {
        let _ = compat::Compat::parse(&text, compat::COMPAT_FILE);
    });
    assert!(
        message.contains("something_new") && message.contains(compat::COMPAT_FILE),
        "the refusal has to name the key and the file, and it said: {message}"
    );
}

#[test]
fn a_key_the_declaration_says_twice_is_refused() {
    let text = format!(
        "{}\nabi_version = 9\n",
        std::fs::read_to_string(compat_path()).expect("COMPAT.toml is missing")
    );
    let message = common::refusal("a duplicated key in COMPAT.toml", || {
        let _ = compat::Compat::parse(&text, compat::COMPAT_FILE);
    });
    assert!(
        message.contains("abi_version") && message.contains("twice"),
        "the refusal has to say which key is said twice, and it said: {message}"
    );
}

#[test]
fn a_key_the_declaration_leaves_out_is_refused() {
    let message = common::refusal("a COMPAT.toml missing a key", || {
        let _ = compat::Compat::parse("abi_version = 2\n", compat::COMPAT_FILE);
    });
    assert!(
        message.contains("wire_version"),
        "the refusal has to name a key that is missing, and it said: {message}"
    );
}

// ---------------------------------------------------------------------------
// Every declared value, against the thing it declares
// ---------------------------------------------------------------------------

#[test]
fn the_declared_versions_are_the_vendored_headers_versions() {
    let declared = declaration();
    let text = common::header_text();
    let name = common::header_name();
    assert_eq!(
        u64::from(declared.abi_version),
        common::header::integer_define(&text, "VIPRS_ACAD_ABI_VERSION", &name),
        "COMPAT.toml's `abi_version` and the header's `VIPRS_ACAD_ABI_VERSION` disagree"
    );
    assert_eq!(
        u64::from(declared.wire_version),
        common::header::integer_define(&text, "VIPRS_ACAD_WIRE_VERSION", &name),
        "COMPAT.toml's `wire_version` and the header's `VIPRS_ACAD_WIRE_VERSION` disagree"
    );
}

#[test]
fn the_declared_versions_are_the_constants_the_crate_compiled_with() {
    let declared = declaration();
    assert_eq!(
        declared.abi_version, EXPECTED_ABI_VERSION,
        "COMPAT.toml declares one ABI version and the crate was built expecting another, which \
         means build.rs did not refuse a disagreement it was supposed to refuse"
    );
    assert_eq!(
        declared.wire_version, EXPECTED_WIRE_VERSION,
        "COMPAT.toml declares one wire version and the crate was built expecting another"
    );
}

#[test]
fn the_declared_digest_is_the_vendored_headers_digest() {
    let declared = declaration();
    let bytes = std::fs::read(common::header_path()).expect("the vendored header is missing");
    assert_eq!(
        declared.abi_header_sha256,
        sha256::hex(&sha256::sha256(&bytes)),
        "COMPAT.toml's `abi_header_sha256` is not the digest of native/viprs_acadsharp.h"
    );
}

#[test]
fn the_declared_digest_is_the_digest_committed_beside_the_header() {
    let declared = declaration();
    let path = repo_root().join("native/viprs_acadsharp.h.sha256");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));
    let committed = text.split_whitespace().next().expect("the pin file is empty");
    assert_eq!(
        declared.abi_header_sha256, committed,
        "COMPAT.toml and native/viprs_acadsharp.h.sha256 describe different headers"
    );
}

// ---------------------------------------------------------------------------
// A wrong declaration stops the build, and says so naming both files
// ---------------------------------------------------------------------------

/// The real declaration with one key's value swapped, which is exactly the
/// shape of the mistake these checks exist for.
fn declaration_with(key: &str, value: &str) -> compat::Compat {
    let text = std::fs::read_to_string(compat_path()).expect("COMPAT.toml is missing");
    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        if line.trim_start().starts_with(key)
            && line.split('=').next().is_some_and(|k| k.trim() == key)
        {
            out.push_str(&format!("{key} = {value}\n"));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(replaced, "COMPAT.toml has no `{key}` line to swap");
    compat::Compat::parse(&out, compat::COMPAT_FILE)
}

#[test]
fn a_wrong_abi_version_stops_the_build_naming_both_files() {
    let declared = declaration_with("abi_version", "77");
    let message = common::refusal("an abi_version that disagrees with the header", || {
        declared.check_against_header(
            EXPECTED_ABI_VERSION,
            EXPECTED_WIRE_VERSION,
            &sha256::hex(&sha256::sha256(
                &std::fs::read(common::header_path()).unwrap(),
            )),
            &common::header_name(),
            compat::COMPAT_FILE,
        );
    });
    assert!(
        message.contains(compat::COMPAT_FILE)
            && message.contains("viprs_acadsharp.h")
            && message.contains("77")
            && message.contains(&EXPECTED_ABI_VERSION.to_string()),
        "the refusal has to name both files and both values, and it said: {message}"
    );
}

#[test]
fn a_wrong_wire_version_stops_the_build_naming_both_files() {
    let declared = declaration_with("wire_version", "88");
    let message = common::refusal("a wire_version that disagrees with the header", || {
        declared.check_against_header(
            EXPECTED_ABI_VERSION,
            EXPECTED_WIRE_VERSION,
            &sha256::hex(&sha256::sha256(
                &std::fs::read(common::header_path()).unwrap(),
            )),
            &common::header_name(),
            compat::COMPAT_FILE,
        );
    });
    assert!(
        message.contains(compat::COMPAT_FILE)
            && message.contains("viprs_acadsharp.h")
            && message.contains("88"),
        "the refusal has to name both files and both values, and it said: {message}"
    );
}

#[test]
fn a_wrong_abi_header_sha256_stops_the_build() {
    // One digit changed, which is the realistic version of this mistake: a
    // digest copied by hand out of a release note, or one left behind when the
    // header moved.
    let real = declaration().abi_header_sha256;
    let mut wrong: Vec<char> = real.chars().collect();
    wrong[0] = if wrong[0] == 'a' { 'b' } else { 'a' };
    let wrong: String = wrong.into_iter().collect();

    let declared = declaration_with("abi_header_sha256", &format!("\"{wrong}\""));
    let message = common::refusal("a digest that is not the header's", || {
        declared.check_against_header(
            EXPECTED_ABI_VERSION,
            EXPECTED_WIRE_VERSION,
            &real,
            &common::header_name(),
            compat::COMPAT_FILE,
        );
    });
    assert!(
        message.contains(compat::COMPAT_FILE)
            && message.contains("viprs_acadsharp.h")
            && message.contains(&wrong)
            && message.contains(&real),
        "the refusal has to name both files and both digests, and it said: {message}"
    );
}

#[test]
fn the_real_declaration_and_the_real_header_agree() {
    // The positive control. Every test above swaps a value and watches a
    // refusal, and a checker that refuses everything passes all of them.
    let declared = declaration();
    declared.check_against_header(
        EXPECTED_ABI_VERSION,
        EXPECTED_WIRE_VERSION,
        &sha256::hex(&sha256::sha256(
            &std::fs::read(common::header_path()).unwrap(),
        )),
        &common::header_name(),
        compat::COMPAT_FILE,
    );
}

// ---------------------------------------------------------------------------
// The artifact-version glob
// ---------------------------------------------------------------------------

#[test]
fn the_glob_accepts_the_artifact_version_that_ships_today() {
    assert!(compat::glob_matches("3.7.1-viprs.*", "3.7.1-viprs.1"));
    assert!(compat::glob_matches("3.7.1-viprs.*", "3.7.1-viprs.12"));
}

#[test]
fn the_glob_refuses_a_different_upstream_version() {
    assert!(!compat::glob_matches("3.7.1-viprs.*", "3.7.2-viprs.1"));
    assert!(!compat::glob_matches("3.7.1-viprs.*", "3.7.11-viprs.1"));
    assert!(!compat::glob_matches("3.7.1-viprs.*", "13.7.1-viprs.1"));
}

/// The case the issue leaves open, decided here and written down in
/// `COMPAT.toml` beside the glob.
///
/// `3.7.1-viprs` has no revision at all, and the glob's trailing `.` is a
/// literal character, so it is **refused**. That is the decision: the revision
/// is part of what an artifact version is, it moves when the shim changes
/// without upstream moving, and an archive claiming to be a version without one
/// is an archive whose provenance I cannot describe. Refusing costs a clear
/// error on a shape nobody publishes; accepting costs a silently linked archive
/// nobody can name.
#[test]
fn the_glob_refuses_an_artifact_version_with_no_revision() {
    assert!(!compat::glob_matches("3.7.1-viprs.*", "3.7.1-viprs"));
}

#[test]
fn the_glob_anchors_at_both_ends() {
    // A substring match would accept all three of these, and a link against any
    // of them is a link against something else entirely.
    assert!(!compat::glob_matches("3.7.1-viprs.*", "x3.7.1-viprs.1"));
    assert!(!compat::glob_matches("3.7.1", "3.7.1-viprs.1"));
    assert!(compat::glob_matches("3.7.1", "3.7.1"));
}

#[test]
fn the_glob_handles_more_than_one_star() {
    assert!(compat::glob_matches("3.*.1-viprs.*", "3.7.1-viprs.1"));
    assert!(!compat::glob_matches("3.*.1-viprs.*", "4.7.1-viprs.1"));
    assert!(compat::glob_matches("*", "anything at all"));
}

#[test]
fn the_declared_glob_accepts_the_archive_that_ships_today() {
    // The declaration's own glob, not a fixture glob, against the artifact
    // version in the real manifest.
    let declared = declaration();
    let identity = compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    assert!(
        compat::glob_matches(&declared.native_artifact_versions, &identity.artifact_version),
        "COMPAT.toml's glob {:?} refuses the artifact version {:?} that ships today",
        declared.native_artifact_versions,
        identity.artifact_version
    );
}

// ---------------------------------------------------------------------------
// The archive, checked against the declaration
// ---------------------------------------------------------------------------

#[test]
fn the_real_archive_manifest_is_accepted() {
    // The other positive control. Two tests below refuse a manifest, and a
    // check that refuses every manifest passes both of them.
    let declared = declaration();
    let identity = compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    declared.check_archive(&identity, &fake_manifest_path(), compat::COMPAT_FILE);
}

#[test]
fn the_manifest_reader_reads_the_two_fields_it_is_for() {
    let identity = compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    assert_eq!(identity.artifact_version, "3.7.1-viprs.1");
    assert_eq!(identity.acadsharp_version, "3.7.1");
}

#[test]
fn an_artifact_version_the_glob_refuses_stops_the_build_naming_compat_toml() {
    let declared = declaration();
    let identity = compat::ArchiveIdentity {
        artifact_version: "3.7.2-viprs.1".to_string(),
        acadsharp_version: "3.7.1".to_string(),
    };
    let message = common::refusal("an archive the glob refuses", || {
        declared.check_archive(&identity, &fake_manifest_path(), compat::COMPAT_FILE);
    });
    assert!(
        message.contains(compat::COMPAT_FILE)
            && message.contains("3.7.2-viprs.1")
            && message.contains(&declared.native_artifact_versions),
        "the refusal has to name COMPAT.toml, the version it got and the glob, and it said: \
         {message}"
    );
}

#[test]
fn an_acadsharp_version_not_in_the_list_stops_the_build_naming_compat_toml() {
    let declared = declaration();
    let identity = compat::ArchiveIdentity {
        artifact_version: "3.7.1-viprs.1".to_string(),
        acadsharp_version: "3.7.15".to_string(),
    };
    let message = common::refusal("an archive built from an unlisted ACadSharp", || {
        declared.check_archive(&identity, &fake_manifest_path(), compat::COMPAT_FILE);
    });
    assert!(
        message.contains(compat::COMPAT_FILE) && message.contains("3.7.15"),
        "the refusal has to name COMPAT.toml and the version it got, and it said: {message}"
    );
}

#[test]
fn a_manifest_with_no_artifact_version_is_refused_rather_than_defaulted() {
    let message = common::refusal("a manifest missing artifact_version", || {
        let _ = compat::ArchiveIdentity::from_manifest_text(
            r#"{ "acadsharp_version": "3.7.1" }"#,
            &fake_manifest_path(),
        );
    });
    assert!(
        message.contains("artifact_version"),
        "the refusal has to name the field it could not find, and it said: {message}"
    );
}

/// The archive this run actually has, when it has one.
///
/// The fixture above keeps the archive-free jobs honest, and this keeps the
/// fixture honest: if the published manifest ever stops looking like the copy
/// in this file, the job with the real archive says so.
#[test]
fn the_archive_on_this_machine_satisfies_the_declaration() {
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        // Not a skip that hides anything: `tests/native_lane_is_live.rs` is
        // what refuses a job that was supposed to have an archive and did not.
        return;
    };
    let manifest = Path::new(&dir).join("metadata").join("LINKINFO.json");
    if !manifest.is_file() {
        return;
    }
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", manifest.display()));
    let identity = compat::ArchiveIdentity::from_manifest_text(&text, &manifest);
    declaration().check_archive(&identity, &manifest, compat::COMPAT_FILE);
    assert_eq!(
        text.replace("\r\n", "\n").trim(),
        REAL_MANIFEST.trim(),
        "the manifest fixture in this test is no longer what the archive ships, so every \
         archive-free job has been checking a file that does not exist any more"
    );
}

// ---------------------------------------------------------------------------
// The README table
// ---------------------------------------------------------------------------

#[test]
fn the_readme_table_is_the_declaration() {
    let declared = declaration();
    let rendered = declared.readme_table();
    let path = repo_root().join("README.md");
    let readme = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));

    if std::env::var_os("ACADSHARP_UPDATE_README").is_some() {
        let updated = compat::replace_readme_table(&readme, &rendered, &path.display().to_string());
        std::fs::write(&path, updated)
            .unwrap_or_else(|e| panic!("I could not write {}: {e}", path.display()));
        return;
    }

    let region = compat::readme_table_region(&readme, &path.display().to_string());
    assert_eq!(
        region.trim(),
        rendered.trim(),
        "the table in README.md and COMPAT.toml disagree. The table is generated, so fix it with \
         `ACADSHARP_UPDATE_README=1 cargo test --test compat readme` rather than by hand."
    );
}

#[test]
fn the_readme_table_names_every_declared_key() {
    // A generated table that quietly dropped a row would still match the README
    // after a regeneration, and then nothing would be checking the row that
    // went missing.
    let declared = declaration();
    let table = declared.readme_table();
    for key in compat::KEYS {
        assert!(
            table.contains(key),
            "the generated README table has no row for `{key}`"
        );
    }
    assert!(table.contains(&declared.abi_header_sha256));
    assert!(table.contains(&declared.native_artifact_versions));
}
