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
///
/// It is a file rather than a string literal so that
/// `tests/compat_build_script.rs`, which lays this manifest out on disk and
/// runs the real build script over it, uses the same bytes and not a second
/// copy that can drift.
const REAL_MANIFEST: &str = include_str!("data/linkinfo/aarch64-unknown-linux-gnu.json");

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
    let committed = text
        .split_whitespace()
        .next()
        .expect("the pin file is empty");
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
    let identity =
        compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    assert!(
        compat::glob_matches(
            &declared.native_artifact_versions,
            &identity.artifact_version
        ),
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
    let identity =
        compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    declared.check_archive(&identity, &fake_manifest_path(), compat::COMPAT_FILE);
}

#[test]
fn the_manifest_reader_reads_the_two_fields_it_is_for() {
    let identity =
        compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
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

/// The archive ships the digest of its own copy of the header, and this crate
/// vendors a copy of the same header. So a vendored header that is not the one
/// the archive was built against shows up as two different digests, and this
/// is the cheapest place that comparison can happen.
///
/// It is a test rather than a build refusal on purpose. Comparing the archive's
/// `abi_header_sha256` against the header the bindings were generated from is
/// step 4 of LINKINFO.md's consumer checklist, and that checklist is the
/// archive policy, which is issue #3's half of `build.rs`. I would rather have
/// the check twice than reach into that half from here, so it lives in the
/// tests until #3's policy picks it up.
#[test]
fn the_declared_digest_is_the_digest_the_archive_says_its_header_has() {
    let declared = declaration();
    assert!(
        REAL_MANIFEST.contains(&declared.abi_header_sha256),
        "COMPAT.toml declares the header digest {} and the published manifest does not carry it,          so the vendored header is not the one the archive was built against",
        declared.abi_header_sha256
    );
}

/// The seam issue #3's parser lands on.
///
/// `check_archive` takes an [`compat::ArchiveIdentity`] and does not care who
/// built it, so the real manifest parser builds one out of its own parsed
/// fields and the hand reader below goes away with no test moving. This is that
/// constructor, checked against the hand reader on the same bytes so the two
/// ways of getting there cannot disagree while both exist.
#[test]
fn an_identity_can_be_built_from_two_strings_without_a_manifest() {
    let read = compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    let built = compat::ArchiveIdentity::new("3.7.1-viprs.1", "3.7.1");
    assert_eq!(
        read, built,
        "the hand reader and the constructor have to produce the same identity, or swapping one \
         for the other at compose time changes what gets checked"
    );
    declaration().check_archive(&built, &fake_manifest_path(), compat::COMPAT_FILE);
}

/// A field name that appears as a value is not a second declaration of that
/// field.
///
/// The hand reader used to count `"artifact_version"` anywhere in the text and
/// refuse a manifest that said it twice, and this manifest is valid JSON that
/// `serde_json` reads without a murmur: an unknown key holding a list of field
/// names. Refusing it stops a build for a reason nobody can act on, which is a
/// worse failure than the one the count was there to catch. The count is at key
/// position now, and a manifest that really does declare the field twice is
/// still refused.
#[test]
fn a_field_name_quoted_inside_a_value_is_not_a_second_declaration() {
    let json = r#"{
  "artifact_version": "3.7.1-viprs.1",
  "acadsharp_version": "3.7.1",
  "fields_this_consumer_reads": ["artifact_version", "acadsharp_version"]
}"#;
    let identity = compat::ArchiveIdentity::from_manifest_text(json, &fake_manifest_path());
    assert_eq!(identity.artifact_version, "3.7.1-viprs.1");
    assert_eq!(identity.acadsharp_version, "3.7.1");
}

#[test]
fn a_manifest_that_really_declares_a_field_twice_is_still_refused() {
    let json = r#"{
  "artifact_version": "3.7.1-viprs.1",
  "artifact_version": "9.9.9-viprs.9",
  "acadsharp_version": "3.7.1"
}"#;
    let message = common::refusal("a manifest declaring artifact_version twice", || {
        let _ = compat::ArchiveIdentity::from_manifest_text(json, &fake_manifest_path());
    });
    assert!(
        message.contains("artifact_version"),
        "the refusal has to name the field that is said twice, and it said: {message}"
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

    // The byte comparison below only means anything against the target the
    // fixture was taken from. `REAL_MANIFEST` is the aarch64 gnu archive, which
    // is what the local Docker gate links; the `Test` job on GitHub is x86_64
    // and links a different archive whose `target`, `platform` and `cpu` are
    // different by construction. Comparing the whole text there conflates two
    // questions, "is the fixture current" and "which archive is on this
    // machine", and it fails on the second while claiming the first.
    //
    // So: the whole file is compared only on the fixture's own target, and
    // everything that is a fact about the release rather than about the target
    // is compared everywhere. The second half is what actually catches a moved
    // pin, because those are the fields a new release changes.
    let fixture_target =
        compat::ArchiveIdentity::from_manifest_text(REAL_MANIFEST, &fake_manifest_path());
    assert_eq!(
        identity.artifact_version, fixture_target.artifact_version,
        "the archive on this machine is {} and the fixture in this test is {}, so the pin moved \
         under the fixture and every archive-free job has been checking a release that is no \
         longer the one CI links",
        identity.artifact_version, fixture_target.artifact_version
    );
    assert_eq!(
        identity.acadsharp_version, fixture_target.acadsharp_version,
        "the archive on this machine reports ACadSharp {} and the fixture reports {}",
        identity.acadsharp_version, fixture_target.acadsharp_version
    );

    if text.contains("\"aarch64-unknown-linux-gnu\"") {
        assert_eq!(
            text.replace("\r\n", "\n").trim(),
            REAL_MANIFEST.trim(),
            "the manifest fixture in this test is no longer what the aarch64 archive ships, so \
             every archive-free job has been checking a file that does not exist any more"
        );
    }
}

// ---------------------------------------------------------------------------
// The archive CI actually fetches, against the declaration
// ---------------------------------------------------------------------------
//
// Everything above this line meets the pin through a downloaded archive, which
// means it only ever runs in the two jobs that download one, and it means a pin
// that never moved looks exactly like a pin that did. Drop `3.7.1` out of
// `acadsharp_versions` and every job carries on proving things about the
// library it fetched while the crate claims a different one.
//
// So the tag and the declaration meet here as two strings in two committed
// files. No network, no archive, and it runs in every job.

/// The composite action both native jobs use to fetch the pinned archive.
const FETCH_ACTION: &str = ".github/actions/fetch-native-archive/action.yml";

/// The release tag is `acadsharp-<artifact_version>`, which is how the two
/// files are comparable at all.
const RELEASE_TAG_PREFIX: &str = "acadsharp-";

/// The `release` input's default out of the fetch action.
///
/// A four-line reader rather than a YAML dependency, and it refuses every shape
/// it was not written for: exactly one `release:` key under `inputs:`, exactly
/// one `default:` inside it. A reader that guessed would be a test that keeps
/// passing after somebody restructures the file, which is the one failure a pin
/// check cannot afford.
fn pinned_release_tag() -> String {
    let path = repo_root().join(FETCH_ACTION);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));

    let indent = |line: &str| line.len() - line.trim_start().len();
    let lines: Vec<&str> = text.lines().collect();

    let heads: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| indent(line) == 2 && line.trim() == "release:")
        .map(|(number, _)| number)
        .collect();
    assert_eq!(
        heads.len(),
        1,
        "{} has {} `release:` keys and this reader wants exactly one",
        path.display(),
        heads.len()
    );

    let mut defaults: Vec<&str> = Vec::new();
    for line in lines.iter().skip(heads[0] + 1) {
        if !line.trim().is_empty() && indent(line) <= 2 {
            break;
        }
        if let Some(value) = line.trim().strip_prefix("default:") {
            defaults.push(value.trim());
        }
    }
    assert_eq!(
        defaults.len(),
        1,
        "the `release` input in {} has {} defaults and this reader wants exactly one",
        path.display(),
        defaults.len()
    );

    let tag = defaults[0].trim_matches('"').trim_matches('\'').to_string();
    assert!(
        !tag.is_empty(),
        "the `release` input in {} has an empty default, which would fetch nothing",
        path.display()
    );
    tag
}

#[test]
fn the_release_ci_pins_is_an_artifact_version_the_declaration_accepts() {
    let declared = declaration();
    let tag = pinned_release_tag();
    let artifact_version = tag.strip_prefix(RELEASE_TAG_PREFIX).unwrap_or_else(|| {
        panic!(
            "the release {tag:?} pinned in {FETCH_ACTION} is not spelled \
             `{RELEASE_TAG_PREFIX}<artifact_version>`, and that spelling is the only thing that \
             makes it comparable with COMPAT.toml's glob"
        )
    });
    assert!(
        compat::glob_matches(&declared.native_artifact_versions, artifact_version),
        "CI fetches {tag:?}, so every native job proves things about the archive \
         {artifact_version:?}, and COMPAT.toml says `native_artifact_versions = {:?}`, which \
         refuses it. One of the two moved without the other. Until they agree, the build refuses \
         the archive its own CI downloads.",
        declared.native_artifact_versions
    );
}

#[test]
fn the_release_ci_pins_is_built_from_a_listed_acadsharp_version() {
    let declared = declaration();
    let tag = pinned_release_tag();
    let artifact_version = tag
        .strip_prefix(RELEASE_TAG_PREFIX)
        .expect("checked in the test above");
    let upstream = artifact_version
        .split_once("-viprs.")
        .map(|(upstream, _)| upstream)
        .unwrap_or_else(|| {
            panic!(
                "the artifact version {artifact_version:?} in {FETCH_ACTION} has no `-viprs.` in \
                 it, so I cannot tell which upstream ACadSharp it was built from"
            )
        });
    assert!(
        declared.acadsharp_versions.iter().any(|v| v == upstream),
        "CI fetches an archive built from ACadSharp {upstream:?} and COMPAT.toml lists \
         `acadsharp_versions = {:?}`. Dropping a version out of that list while CI still fetches \
         it is the quiet half of this mistake: every job carries on proving things about the \
         library it downloaded, and the crate claims a different one.",
        declared.acadsharp_versions
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
    // Row by row rather than `assert_eq!` on the two whole tables. A failure
    // that prints two escaped multi-line blobs and leaves the reader to spot
    // the difference is a failure nobody reviews, and I have watched exactly
    // that happen on a gate that compared two JSON documents this way.
    let found: Vec<&str> = region.trim().lines().map(str::trim_end).collect();
    let want: Vec<&str> = rendered.trim().lines().map(str::trim_end).collect();
    for (number, (found, want)) in found.iter().zip(want.iter()).enumerate() {
        assert_eq!(
            found,
            want,
            "line {} of the generated table in README.md is not what COMPAT.toml says.\n  README.md:   {found}\n  COMPAT.toml: {want}\nRegenerate it with `ACADSHARP_UPDATE_README=1 cargo test --test compat readme` rather than editing it by hand.",
            number + 1
        );
    }
    assert_eq!(
        found.len(),
        want.len(),
        "the generated table in README.md has {} lines and COMPAT.toml renders {}. Every line they share matches, so this is a row added or dropped by hand. Regenerate it with `ACADSHARP_UPDATE_README=1 cargo test --test compat readme`.",
        found.len(),
        want.len()
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
