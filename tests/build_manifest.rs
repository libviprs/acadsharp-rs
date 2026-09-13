//! `metadata/LINKINFO.json`, validated.
//!
//! This file holds the half of the manifest reader that needs no JSON parser:
//! the rules about which fields have to be there, what a value is allowed to
//! look like, and which combinations of the four static fields are a manifest
//! rather than a mistake. `build/manifest.rs` is compiled in here directly, the
//! same trick `tests/abi_constants.rs` uses on `build/sha256.rs`, because a
//! build script's own `#[cfg(test)]` module is compiled by nothing and a test
//! that never runs looks exactly like a test that passed.
//!
//! The JSON layer sits in `build/linkinfo.rs` on top of `serde_json`, which is
//! a `[build-dependencies]` entry and so is not available to a test binary.
//! That layer is driven end to end in `tests/build_script.rs` instead, by
//! running the real build script over the two real manifests and reading the
//! directives that come out. So both halves are covered, and neither is covered
//! by a copy of itself.
//!
//! # Everything in here ends up in a cargo directive, so everything in here is
//! checked
//!
//! The manifest arrives inside a downloaded tarball, and cargo's build-script
//! protocol is one directive per line of stdout. So a newline inside a string
//! this reader repeats is not a broken name, it is the end of one directive and
//! the start of another one the tarball chose. Proven end to end before this
//! check existed: a `shared_system_libraries` entry of
//! `"m\ncargo::rustc-link-arg=..."` put an arbitrary flag on the real link
//! line, and a newline in `ACADSHARP_NATIVE_DIR` split the `cargo::warning=`
//! line the same way.

use std::path::{Path, PathBuf};

#[path = "../build/manifest.rs"]
mod manifest;

use manifest::{LinkInfoError, Raw};

/// What a refusal has to call the file, so a message says where to go and look.
fn manifest_path() -> PathBuf {
    PathBuf::from("/somewhere/unpacked/acadsharp-linux-arm64/metadata/LINKINFO.json")
}

/// The shipped `aarch64-unknown-linux-gnu` manifest, field by field.
///
/// Every value is the real archive's value. It is typed out here rather than
/// parsed because this file deliberately owns no JSON parser, and the JSON that
/// produces exactly this is checked against the real file in
/// `tests/build_script.rs`. `the_field_list_matches_both_shipped_manifests`
/// below is the guard that stops this drifting from the real key set.
fn certified() -> Raw {
    Raw {
        schema_version: Some(1),
        artifact_version: Some("3.7.1-viprs.1".into()),
        acadsharp_version: Some("3.7.1".into()),
        acadsharp_commit: Some("d7dc111023477d8a9fffc2153139459c95b4f345".into()),
        dotnet_sdk: Some("10.0.401".into()),
        target: Some("aarch64-unknown-linux-gnu".into()),
        platform: Some("linux".into()),
        cpu: Some("arm64".into()),
        abi_version: Some(2),
        wire_version: Some(2),
        abi_header_sha256: Some(
            "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa".into(),
        ),
        abi_fingerprint: Some("0502ac0f61611530".into()),
        shared_library: Some("lib/libacadsharp_native.so".into()),
        shared_system_libraries: Some(vec!["m".into()]),
        static_library: Some("lib/libacadsharp_native.a".into()),
        static_init_library: Some("lib/libacadsharp_native_init.a".into()),
        static_certified: Some(true),
        static_system_libraries: Some(vec!["m".into()]),
        static_link_args: Some(vec![]),
        dwg_version_min: Some(1014),
        dwg_version_max: Some(1032),
    }
}

/// The shipped `aarch64-apple-darwin` manifest, which ships no static half.
fn uncertified() -> Raw {
    Raw {
        target: Some("aarch64-apple-darwin".into()),
        platform: Some("mac".into()),
        cpu: Some("arm64".into()),
        shared_library: Some("lib/libacadsharp_native.dylib".into()),
        shared_system_libraries: Some(vec![
            "icucore.A".into(),
            "objc.A".into(),
            "swiftCore".into(),
            "swiftFoundation".into(),
            "System.B".into(),
        ]),
        static_library: None,
        static_init_library: None,
        static_certified: Some(false),
        static_system_libraries: None,
        static_link_args: None,
        ..certified()
    }
}

fn validate(raw: Raw) -> Result<manifest::LinkInfo, LinkInfoError> {
    raw.validate(&manifest_path())
}

/// One row of the tables below: the field this case is about, and the edit
/// that makes its rule fire.
type Case = (&'static str, Box<dyn Fn(&mut Raw)>);

/// The same thing for a table that has a value to put in as well as a field to
/// put it in.
type Setter = Box<dyn Fn(&mut Raw, String)>;

fn refused(raw: Raw, what: &str) -> LinkInfoError {
    match validate(raw) {
        Ok(info) => panic!("I expected {what} to be refused, and it parsed as {info:?}"),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// The positive controls, first, because a validator that refuses everything
// passes every refusal test below and links nothing.
// ---------------------------------------------------------------------------

#[test]
fn the_certified_manifest_carries_a_static_half() {
    let info = validate(certified()).expect("the shipped linux manifest is a manifest");
    assert_eq!(info.target, "aarch64-unknown-linux-gnu");
    assert_eq!(info.abi_version, 2);
    assert_eq!(info.wire_version, 2);
    assert_eq!(info.dwg_version_min, 1014);
    assert_eq!(info.dwg_version_max, 1032);
    assert_eq!(info.shared_library_stem, "acadsharp_native");
    assert_eq!(info.shared_system_libraries, vec!["m".to_string()]);

    let statics = info.statics.expect("this archive is certified");
    assert_eq!(statics.library_stem, "acadsharp_native");
    assert_eq!(statics.init_library_stem, "acadsharp_native_init");
    assert_eq!(statics.system_libraries, vec!["m".to_string()]);
    assert!(statics.link_args.is_empty());
}

#[test]
fn the_uncertified_manifest_carries_no_static_half_at_all() {
    let info = validate(uncertified()).expect("the shipped mac manifest is a manifest");
    assert_eq!(info.target, "aarch64-apple-darwin");
    assert_eq!(info.shared_library_stem, "acadsharp_native");
    assert_eq!(info.shared_system_libraries.len(), 5);
    assert!(
        info.statics.is_none(),
        "`static_certified: false` has to mean there is no static half to reach for, not an \
         empty one somebody can still index into"
    );
}

#[test]
fn the_field_list_matches_both_shipped_manifests() {
    // The guard on the two hand-typed fixtures above. If the archive gains,
    // renames or drops a key, this fails here rather than in a build that
    // quietly read one field fewer than the archive shipped.
    for fixture in ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let text = fixture_text(fixture);
        for key in top_level_keys(&text) {
            assert!(
                manifest::KNOWN_FIELDS.contains(&key.as_str()),
                "{fixture}.json ships a `{key}` field that the manifest reader has never heard \
                 of. An unknown key is allowed to be skipped, but a shipped one that nobody \
                 wired up is a field this build script is ignoring on purpose it never made."
            );
        }
    }

    // And the other direction, on the certified one, which ships every field
    // there is. A reader that knows about a field no archive has is a reader
    // built from prose rather than from an archive.
    let text = fixture_text("aarch64-unknown-linux-gnu");
    let shipped = top_level_keys(&text);
    for known in manifest::KNOWN_FIELDS {
        assert!(
            shipped.iter().any(|k| k == known),
            "the reader knows a `{known}` field and the shipped certified manifest has no such \
             key, so that field came from somewhere other than an archive"
        );
    }
}

fn fixture_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/linkinfo")
        .join(format!("{name}.json"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()))
}

/// The `"name":` keys of a flat JSON object, found by indentation.
///
/// Good enough for exactly these two files, which the archive writes with two
/// spaces of indentation and no nesting beyond arrays of strings, and this is a
/// test rather than the parser.
fn top_level_keys(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  \"")?;
            let (name, after) = rest.split_once('"')?;
            after.starts_with(':').then(|| name.to_string())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// A missing required field, one test per field
// ---------------------------------------------------------------------------

#[test]
fn every_required_field_that_goes_missing_is_its_own_refusal() {
    // One closure per field rather than a reflective loop, because the point is
    // that each one is named in its own message. A loop over field names would
    // need the reader to expose a by-name setter, and then the test would be
    // checking the setter.
    let cases: Vec<Case> = vec![
        (
            "schema_version",
            Box::new(|r: &mut Raw| r.schema_version = None),
        ),
        (
            "artifact_version",
            Box::new(|r: &mut Raw| r.artifact_version = None),
        ),
        (
            "acadsharp_version",
            Box::new(|r: &mut Raw| r.acadsharp_version = None),
        ),
        (
            "acadsharp_commit",
            Box::new(|r: &mut Raw| r.acadsharp_commit = None),
        ),
        ("dotnet_sdk", Box::new(|r: &mut Raw| r.dotnet_sdk = None)),
        ("target", Box::new(|r: &mut Raw| r.target = None)),
        ("platform", Box::new(|r: &mut Raw| r.platform = None)),
        ("cpu", Box::new(|r: &mut Raw| r.cpu = None)),
        ("abi_version", Box::new(|r: &mut Raw| r.abi_version = None)),
        (
            "wire_version",
            Box::new(|r: &mut Raw| r.wire_version = None),
        ),
        (
            "dwg_version_min",
            Box::new(|r: &mut Raw| r.dwg_version_min = None),
        ),
        (
            "dwg_version_max",
            Box::new(|r: &mut Raw| r.dwg_version_max = None),
        ),
        (
            "abi_header_sha256",
            Box::new(|r: &mut Raw| r.abi_header_sha256 = None),
        ),
        (
            "abi_fingerprint",
            Box::new(|r: &mut Raw| r.abi_fingerprint = None),
        ),
        (
            "shared_library",
            Box::new(|r: &mut Raw| r.shared_library = None),
        ),
        (
            "shared_system_libraries",
            Box::new(|r: &mut Raw| r.shared_system_libraries = None),
        ),
        (
            "static_certified",
            Box::new(|r: &mut Raw| r.static_certified = None),
        ),
    ];

    for (field, drop_it) in cases {
        let mut raw = certified();
        drop_it(&mut raw);
        let error = refused(raw, &format!("a manifest with no `{field}`"));
        assert_eq!(
            error,
            LinkInfoError::MissingField {
                manifest: manifest_path().display().to_string(),
                field,
            },
            "dropping `{field}` has to be its own refusal naming that field, and it said: {error}"
        );
        let shown = error.to_string();
        assert!(
            shown.contains(field) && shown.contains("LINKINFO.json"),
            "the message for a missing `{field}` has to name the field and the manifest, and it \
             said: {shown}"
        );
    }
}

// ---------------------------------------------------------------------------
// schema_version
// ---------------------------------------------------------------------------

#[test]
fn a_schema_version_from_the_future_refuses_the_whole_archive() {
    let raw = Raw {
        schema_version: Some(2),
        ..certified()
    };
    let error = refused(raw, "a schema_version of 2");
    assert_eq!(
        error,
        LinkInfoError::SchemaVersionTooNew {
            manifest: manifest_path().display().to_string(),
            found: 2,
            known: manifest::KNOWN_SCHEMA_VERSION,
        },
        "a higher schema version is a refusal of the archive, not a reason to press on with the \
         fields I recognise: it means a field I am reading may no longer mean what it meant, and \
         the failures that come of guessing are link-time or run-time crashes in a downstream \
         binary a long way from here"
    );
}

#[test]
fn an_older_schema_version_is_read_as_long_as_every_field_is_there() {
    // The other half of the rule, and the reason the first half is safe.
    // Within a schema version fields are only ever added in a way that keeps
    // the existing ones meaning what they meant, so a lower number with every
    // field present is a manifest this reader can describe.
    let raw = Raw {
        schema_version: Some(0),
        ..certified()
    };
    let info = validate(raw).expect("an older schema version with every field present reads");
    assert_eq!(info.schema_version, 0);
}

// ---------------------------------------------------------------------------
// The four static fields move together, and absent is not empty
// ---------------------------------------------------------------------------

#[test]
fn certified_with_any_static_field_missing_is_refused_by_name() {
    let cases: Vec<Case> = vec![
        (
            "static_library",
            Box::new(|r: &mut Raw| r.static_library = None),
        ),
        (
            "static_init_library",
            Box::new(|r: &mut Raw| r.static_init_library = None),
        ),
        (
            "static_system_libraries",
            Box::new(|r: &mut Raw| r.static_system_libraries = None),
        ),
        (
            "static_link_args",
            Box::new(|r: &mut Raw| r.static_link_args = None),
        ),
    ];

    for (field, drop_it) in cases {
        let mut raw = certified();
        drop_it(&mut raw);
        let error = refused(raw, &format!("`static_certified: true` with no `{field}`"));
        assert_eq!(
            error,
            LinkInfoError::StaticFieldMissing {
                manifest: manifest_path().display().to_string(),
                field,
            },
            "all four static fields move together, so `{field}` going missing under \
             `static_certified: true` is its own refusal. It said: {error}"
        );
    }
}

#[test]
fn a_static_field_beside_static_certified_false_is_refused_by_name() {
    // The shape a build script author reads as belonging to the shared link.
    // There is no third state, so a field describing a static link that was
    // never certified is a manifest defect rather than a hint.
    let cases: Vec<Case> = vec![
        (
            "static_library",
            Box::new(|r: &mut Raw| r.static_library = Some("lib/libacadsharp_native.a".into())),
        ),
        (
            "static_init_library",
            Box::new(|r: &mut Raw| {
                r.static_init_library = Some("lib/libacadsharp_native_init.a".into());
            }),
        ),
        (
            "static_system_libraries",
            Box::new(|r: &mut Raw| r.static_system_libraries = Some(vec!["m".into()])),
        ),
        (
            "static_link_args",
            Box::new(|r: &mut Raw| r.static_link_args = Some(vec![])),
        ),
    ];

    for (field, add_it) in cases {
        let mut raw = uncertified();
        add_it(&mut raw);
        let error = refused(
            raw,
            &format!("`static_certified: false` carrying `{field}`"),
        );
        assert_eq!(
            error,
            LinkInfoError::StaticFieldUnexpected {
                manifest: manifest_path().display().to_string(),
                field,
            },
            "a `{field}` beside `static_certified: false` is refused by name, and it said: {error}"
        );
    }
}

#[test]
fn an_empty_static_library_path_is_malformed_rather_than_absent() {
    // Absent is not empty. An empty string is a path, an empty array is a
    // measurement of none, and neither of those means "not measured", so the
    // reader tests for the key rather than for truthiness of the value.
    let raw = Raw {
        static_library: Some(String::new()),
        ..certified()
    };
    let error = refused(raw, "`\"static_library\": \"\"`");
    assert_eq!(
        error,
        LinkInfoError::EmptyField {
            manifest: manifest_path().display().to_string(),
            field: "static_library",
        }
    );
}

#[test]
fn an_empty_static_system_libraries_array_is_a_measurement_and_is_fine() {
    // The positive control for the rule above, and the difference the rule is
    // about: an empty array measured none, which is a fact, while an empty
    // string is a path nobody wrote.
    let raw = Raw {
        static_system_libraries: Some(vec![]),
        ..certified()
    };
    let info = validate(raw).expect("an archive that needed no system libraries is an archive");
    let statics = info.statics.expect("still certified");
    assert!(statics.system_libraries.is_empty());
}

// ---------------------------------------------------------------------------
// abi_fingerprint has a format, and the format is load-bearing
// ---------------------------------------------------------------------------

#[test]
fn the_fingerprint_is_kept_as_a_number() {
    let info = validate(certified()).expect("the shipped manifest parses");
    assert_eq!(
        info.abi_fingerprint, 0x0502_ac0f_6161_1530,
        "the fingerprint means a number and is a string in the JSON only because 64 unsigned bits \
         do not survive every JSON number reader intact. Comparing the text instead makes the \
         check depend on whether somebody's formatter padded a leading zero."
    );
}

#[test]
fn every_way_of_writing_the_fingerprint_wrong_is_refused() {
    let cases = [
        (
            "0x0502ac0f61611530",
            "a 0x prefix, which most base-16 parsers reject rather than skip",
        ),
        ("0502AC0F61611530", "uppercase"),
        (
            "502ac0f61611530",
            "fifteen characters, so a leading zero went missing",
        ),
        ("0502ac0f616115300", "seventeen characters"),
        ("", "nothing at all"),
        ("0502ac0f6161153g", "a character that is not a hex digit"),
        (" 0502ac0f61611530", "leading whitespace"),
    ];

    for (value, why) in cases {
        let raw = Raw {
            abi_fingerprint: Some(value.to_string()),
            ..certified()
        };
        let error = refused(raw, &format!("a fingerprint with {why}"));
        assert!(
            matches!(error, LinkInfoError::Fingerprint { .. }),
            "a fingerprint with {why} has to be refused as a fingerprint, and it said: {error}"
        );
        assert!(
            error.to_string().contains("LINKINFO.json"),
            "the refusal has to name the manifest, and it said: {error}"
        );
    }
}

#[test]
fn a_fingerprint_that_is_not_the_head_of_the_digest_beside_it_is_refused() {
    // The relationship LINKINFO.md says a consumer checks: the fingerprint is
    // the first eight bytes of `abi_header_sha256`, read big-endian. Two
    // fields that are supposed to agree and do not is a manifest whose
    // producer did something by hand.
    let raw = Raw {
        abi_fingerprint: Some("0502ac0f61611531".into()),
        ..certified()
    };
    let error = refused(raw, "a fingerprint that is not the head of the digest");
    assert!(
        matches!(error, LinkInfoError::FingerprintDoesNotMatchDigest { .. }),
        "it said: {error}"
    );
}

#[test]
fn a_header_digest_that_is_not_64_lowercase_hex_is_refused() {
    for value in [
        "0502AC0F",
        "",
        "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233f",
    ] {
        let raw = Raw {
            abi_header_sha256: Some(value.to_string()),
            ..certified()
        };
        let error = refused(raw, "a malformed abi_header_sha256");
        assert!(
            matches!(error, LinkInfoError::HeaderDigest { .. }),
            "{value:?} said: {error}"
        );
    }
}

// ---------------------------------------------------------------------------
// Library names and library paths, which are what reaches a cargo directive
// ---------------------------------------------------------------------------

#[test]
fn a_library_name_carrying_cargo_directives_is_refused() {
    // The review's own injection, which was proven end to end: it produced
    // those exact directives in the build script's output and the flag reached
    // the real link line.
    let raw = Raw {
        shared_system_libraries: Some(vec![
            "m\ncargo::rustc-link-arg=--totally-bogus-linker-flag\ncargo::rustc-env=PWNED=yes"
                .into(),
        ]),
        ..certified()
    };
    let error = refused(raw, "a library name with cargo directives in it");
    assert!(
        matches!(error, LinkInfoError::LibraryName { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("shared_system_libraries") && shown.contains("LINKINFO.json"),
        "a refusal has to name the field and the manifest, and it said: {shown}"
    );
}

#[test]
fn every_character_a_library_name_may_not_have_is_refused() {
    // One entry per reason a name gets rejected. A check written for a newline
    // alone passes the first case and misses every other way of saying it.
    let cases = [
        ("m\ncargo::rustc-env=PWNED=yes", "a newline and a directive"),
        ("m\tcargo::rustc-env=PWNED=yes", "a tab"),
        ("m\u{7f}", "a DEL"),
        ("acadsharp native", "a space"),
        ("../../../etc/passwd", "a path separator"),
        ("m;id", "a semicolon"),
        ("", "nothing at all"),
    ];

    for (name, why) in cases {
        for field in ["shared_system_libraries", "static_system_libraries"] {
            let mut raw = certified();
            if field == "shared_system_libraries" {
                raw.shared_system_libraries = Some(vec![name.to_string()]);
            } else {
                raw.static_system_libraries = Some(vec![name.to_string()]);
            }
            let error = refused(raw, &format!("{field} carrying {why}"));
            assert!(
                error.to_string().contains(field),
                "the refusal for {why} in {field} has to name that field, and it said: {error}"
            );
        }
    }
}

#[test]
fn the_names_the_real_archive_ships_are_all_accepted() {
    // The positive control. `icucore.A` and `System.B` are real entries out of
    // the shipped mac manifest, and a name check tight enough to refuse them
    // would refuse a real archive.
    let info = validate(uncertified()).expect("the shipped mac names are names");
    assert_eq!(
        info.shared_system_libraries,
        vec![
            "icucore.A",
            "objc.A",
            "swiftCore",
            "swiftFoundation",
            "System.B"
        ]
    );

    // And the ones a static link on a glibc host would plausibly list.
    let raw = Raw {
        static_system_libraries: Some(vec![
            "m".into(),
            "rt".into(),
            "dl".into(),
            "pthread".into(),
            "stdc++".into(),
            "gcc_s".into(),
            "c++abi".into(),
        ]),
        ..certified()
    };
    let info = validate(raw).expect("ordinary system library names are names");
    assert_eq!(info.statics.expect("certified").system_libraries.len(), 7);
}

#[test]
fn the_library_stems_come_out_of_the_manifests_paths() {
    // The `lib` prefix and the extension stripped, which is the same
    // transformation `-l` has always done. Derived from the manifest rather
    // than from a constant in this crate, so an archive that renames its
    // libraries keeps working.
    let raw = Raw {
        shared_library: Some("lib/libwibble.so".into()),
        static_library: Some("lib/libwibble.a".into()),
        static_init_library: Some("lib/libwibble_init.a".into()),
        ..certified()
    };
    let info = validate(raw).expect("a renamed library is still a library");
    assert_eq!(info.shared_library_stem, "wibble");
    let statics = info.statics.expect("certified");
    assert_eq!(statics.library_stem, "wibble");
    assert_eq!(statics.init_library_stem, "wibble_init");
}

#[test]
fn a_library_path_that_leaves_the_archive_is_refused() {
    let cases = [
        ("/usr/lib/libevil.so", "an absolute path"),
        ("../../libevil.so", "a parent directory"),
        ("lib/../../libevil.so", "a parent directory in the middle"),
        ("lib/libevil.so\ncargo::rustc-link-arg=-w", "a directive"),
        ("lib/evil.so", "no lib prefix on the file name"),
        ("lib/libevil", "no extension at all"),
        ("lib/lib.so", "no stem behind the lib prefix"),
    ];

    for (path, why) in cases {
        let raw = Raw {
            shared_library: Some(path.to_string()),
            ..certified()
        };
        let error = refused(raw, &format!("a shared_library that is {why}"));
        assert!(
            matches!(error, LinkInfoError::LibraryPath { .. }),
            "a shared_library that is {why} said: {error}"
        );
        assert!(
            error.to_string().contains("shared_library"),
            "the refusal for {why} has to name the field, and it said: {error}"
        );
    }
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[test]
fn a_number_too_big_for_the_field_it_describes_is_refused() {
    let raw = Raw {
        abi_version: Some(u64::from(u32::MAX) + 1),
        ..certified()
    };
    let error = refused(raw, "an abi_version past u32");
    assert!(
        matches!(error, LinkInfoError::NumberOutOfRange { .. }),
        "it said: {error}"
    );
}

#[test]
fn a_dwg_range_that_runs_backwards_is_refused() {
    let raw = Raw {
        dwg_version_min: Some(1032),
        dwg_version_max: Some(1014),
        ..certified()
    };
    let error = refused(raw, "a dwg range that runs backwards");
    assert!(
        matches!(error, LinkInfoError::DwgRange { .. }),
        "the manifest says `dwg_version_max` is never below `dwg_version_min`, and it said: {error}"
    );
}

// ---------------------------------------------------------------------------
// The archive root itself, which reaches cargo before any manifest is read
// ---------------------------------------------------------------------------

#[test]
fn an_archive_root_with_a_newline_in_it_is_refused() {
    // The same injection one layer out. `ACADSHARP_NATIVE_DIR` reaches cargo
    // through the link-search line, through the rpath and through the warning
    // that says no archive resolved, and this one needs no manifest at all.
    let root = PathBuf::from("/tmp/nope\ncargo::rustc-env=PWNED_BY_THE_PATH=yes");
    let manifest = root.join("metadata").join("LINKINFO.json");
    let error = manifest::check_archive_root(&root, &manifest)
        .expect_err("a root with a newline in it is refused");
    assert!(
        error.to_string().contains("LINKINFO.json"),
        "a refusal has to name the manifest it would have read, and it said: {error}"
    );
}

#[test]
fn an_ordinary_archive_root_is_not_refused() {
    // The positive control. A path with a space in it is a perfectly good path
    // and survives a single directive line, so refusing it would break
    // somebody's checkout for no reason.
    let root = PathBuf::from("/home/someone/My Archives/acadsharp-linux-arm64");
    manifest::check_archive_root(&root, &root.join("metadata").join("LINKINFO.json"))
        .expect("a path with a space in it is a path");
}

// ---------------------------------------------------------------------------
// The seven plain string fields, which reach cargo the same way the names do
// ---------------------------------------------------------------------------

/// The seven fields nothing used to look at, and the edit that poisons each.
///
/// `artifact_version` is the live one, because `build.rs` prints
/// `cargo::metadata=artifact_version={}` and cargo turns that into
/// `DEP_ACADSHARP_NATIVE_ARTIFACT_VERSION` for every downstream build script.
/// The rest are provenance, and provenance still lands in a refusal message
/// that somebody's terminal renders.
fn plain_string_fields() -> Vec<(&'static str, Setter)> {
    let fields: Vec<(&'static str, Setter)> = vec![
        (
            "artifact_version",
            Box::new(|raw: &mut Raw, v: String| raw.artifact_version = Some(v)),
        ),
        (
            "acadsharp_version",
            Box::new(|raw: &mut Raw, v: String| raw.acadsharp_version = Some(v)),
        ),
        (
            "acadsharp_commit",
            Box::new(|raw: &mut Raw, v: String| raw.acadsharp_commit = Some(v)),
        ),
        (
            "dotnet_sdk",
            Box::new(|raw: &mut Raw, v: String| raw.dotnet_sdk = Some(v)),
        ),
        (
            "target",
            Box::new(|raw: &mut Raw, v: String| raw.target = Some(v)),
        ),
        (
            "platform",
            Box::new(|raw: &mut Raw, v: String| raw.platform = Some(v)),
        ),
        ("cpu", Box::new(|raw: &mut Raw, v: String| raw.cpu = Some(v))),
    ];
    fields
}

#[test]
fn a_control_character_in_any_plain_string_field_is_refused_by_name() {
    // Measured against the reader before this check existed: a manifest whose
    // `artifact_version` was `"3.7.1-viprs.1\ncargo::rustc-env=PWNED=yes"`
    // validated clean and the build script printed
    //
    //     cargo::metadata=artifact_version=3.7.1-viprs.1
    //     cargo::rustc-env=PWNED=yes
    //
    // as two directives, the second one chosen by the tarball. The library
    // name check next door was thorough and simply never looked at these seven.
    for (field, set) in plain_string_fields() {
        for (value, why) in [
            ("3.7.1-viprs.1\ncargo::rustc-env=PWNED=yes", "a newline"),
            ("3.7.1-viprs.1\rcargo::rustc-env=PWNED=yes", "a carriage return"),
            ("3.7.1\tviprs", "a tab"),
            ("3.7.1\u{7f}", "a DEL"),
            ("3.7.1\u{0}", "a NUL"),
        ] {
            let mut raw = certified();
            set(&mut raw, value.to_string());
            let error = refused(raw, &format!("{field} carrying {why}"));
            let shown = error.to_string();
            assert!(
                shown.contains(field),
                "the refusal for {why} in {field} has to name that field, and it said: {shown}"
            );
            assert!(
                shown.contains("LINKINFO.json"),
                "and it has to name the manifest, and it said: {shown}"
            );
        }
    }
}

#[test]
fn the_glob_in_a_version_pin_is_no_defence_here() {
    // Worth stating because it is the reason this check is not somebody else's
    // problem. A compatibility pin matching `artifact_version` against
    // `3.7.1-viprs.*` is a glob, and `*` matches a newline, so a poisoned
    // version string sails through the pin and out the other side.
    let poisoned = "3.7.1-viprs.1\ncargo::rustc-env=PWNED=yes";
    assert!(
        poisoned.starts_with("3.7.1-viprs."),
        "which is all a `3.7.1-viprs.*` glob ever asks"
    );
    let mut raw = certified();
    raw.artifact_version = Some(poisoned.to_string());
    refused(raw, "a version that a glob pin would happily match");
}

#[test]
fn the_values_the_real_archives_carry_in_those_seven_fields_are_accepted() {
    // The positive control. A check that refused a real manifest would pass
    // every case above and link nothing.
    for raw in [certified(), uncertified()] {
        validate(raw).expect("the shipped manifests are manifests");
    }
    // And the characters that are not control characters stay legal: these are
    // paths, versions and triples, not library names, so `.`, `-` and `+` are
    // all ordinary here.
    let raw = Raw {
        artifact_version: Some("3.7.1-viprs.1+build.5".into()),
        dotnet_sdk: Some("10.0.401-preview.2".into()),
        ..certified()
    };
    validate(raw).expect("a version with a build metadata suffix is a version");
}
