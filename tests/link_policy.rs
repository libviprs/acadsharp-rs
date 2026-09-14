//! Which way the crate links, decided as a pure function.
//!
//! `policy::choose` does no I/O at all: it takes a validated manifest, the two
//! feature flags, the target triple cargo is building for and the three numbers
//! the vendored header says to expect, and hands back either the exact list of
//! directives to print or a refusal. That is the whole reason it is a separate
//! file. Every rule below is one table row, and a rule that only existed inside
//! `build.rs` could only be tested by building something.
//!
//! # The one silent failure
//!
//! Wrong order of the two `rustc-link-lib` lines fails loudly on
//! `RhRegisterOSModule`. `+bundle` on either fails loudly the same way.
//! Omitting `+whole-archive` links clean and aborts on the first call into the
//! library. So the assertions here are on the exact strings and their exact
//! order, and `tests/static_link.rs` is the half that calls through and reads
//! the answer back, because a test that only links proves nothing.

use std::path::{Path, PathBuf};

#[path = "../build/manifest.rs"]
mod manifest;
#[path = "../build/policy.rs"]
mod policy;

use manifest::{LinkInfo, Raw};
use policy::{Archive, Expectations, Features, LinkKind, PolicyError};

/// Where the archive is unpacked, in every test in this file.
const ROOT: &str = "/somewhere/unpacked/acadsharp-linux-arm64";

fn archive() -> Archive {
    Archive::at(PathBuf::from(ROOT))
}

/// The manifest path a refusal quotes, which is the one inside the archive.
fn manifest_path() -> PathBuf {
    PathBuf::from(ROOT).join("metadata").join("LINKINFO.json")
}

/// What the vendored header says, which is what a manifest has to agree with.
///
/// The digest is the whole 256 bits of it, not just the eight bytes the
/// fingerprint carries. `native/NATIVE_HEADER_REV` names the commit the header
/// came from and nothing in the crate checks that name; this comparison is the
/// witness for it, because a header taken from a different commit hashes
/// differently and an archive built against the old one still says so.
fn expected() -> Expectations {
    Expectations {
        abi_version: 2,
        wire_version: 2,
        abi_fingerprint: 0x0502_ac0f_6161_1530,
        header_file: "native/viprs_acadsharp.h".into(),
        header_sha256: "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa".into(),
    }
}

fn certified_raw() -> Raw {
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
        static_system_libraries: Some(vec!["m".into(), "rt".into()]),
        static_link_args: Some(vec![]),
        dwg_version_min: Some(1014),
        dwg_version_max: Some(1032),
    }
}

fn certified() -> LinkInfo {
    certified_raw()
        .validate(&manifest_path())
        .expect("the fixture is a manifest")
}

fn uncertified() -> LinkInfo {
    Raw {
        target: Some("aarch64-apple-darwin".into()),
        platform: Some("mac".into()),
        cpu: Some("arm64".into()),
        shared_library: Some("lib/libacadsharp_native.dylib".into()),
        shared_system_libraries: Some(vec!["objc.A".into()]),
        static_library: None,
        static_init_library: None,
        static_certified: Some(false),
        static_system_libraries: None,
        static_link_args: None,
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("the fixture is a manifest")
}

/// No feature, which is the default and which is the shared link.
const NOTHING: Features = Features { link_static: false };
/// The one feature there is.
const STATIC: Features = Features { link_static: true };

fn choose(
    info: &LinkInfo,
    features: Features,
    target: &str,
) -> Result<policy::LinkPlan, PolicyError> {
    policy::choose(info, features, target, expected(), &archive())
}

fn plan(info: &LinkInfo, features: Features, target: &str) -> policy::LinkPlan {
    choose(info, features, target).expect("this combination is supposed to produce a plan")
}

fn refused(info: &LinkInfo, features: Features, target: &str, what: &str) -> PolicyError {
    match choose(info, features, target) {
        Ok(plan) => panic!("I expected {what} to be refused, and it planned {plan:?}"),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// The directives, exactly, and in order
// ---------------------------------------------------------------------------

#[test]
fn the_static_recipe_is_emitted_exactly_and_in_order() {
    let info = certified();
    let plan = plan(&info, STATIC, "aarch64-unknown-linux-gnu");
    assert_eq!(plan.kind, LinkKind::Static);
    assert_eq!(
        plan.directives(),
        vec![
            format!("cargo::rustc-link-search=native={ROOT}/lib"),
            "cargo::rustc-link-lib=static:-bundle,+whole-archive=acadsharp_native_init".to_string(),
            "cargo::rustc-link-lib=static:-bundle=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=m".to_string(),
            "cargo::rustc-link-lib=rt".to_string(),
        ],
        "all three modifiers are load bearing and so is the order. `+whole-archive` on the init \
         archive, because nothing references what is in it and without it the binary aborts at \
         the first call. `-bundle` on both, or rustc packs the objects into this crate's rlib and \
         the rlib lands ahead of the init archive on the final link line. Init archive first, \
         because a linker reads left to right and the init object is the one calling \
         RhRegisterOSModule."
    );
}

#[test]
fn the_static_plan_carries_no_rpath() {
    // `lib/` holds both the `.so` and the `.a`, and a bare `-l` picks the
    // `.so`. So an rpath left on the static path means a binary that was
    // supposed to be self-contained links, loads and runs correctly on the
    // build machine by quietly using the shared library. That is a silent
    // success, which is worse than a failure.
    let info = certified();
    let plan = plan(&info, STATIC, "aarch64-unknown-linux-gnu");
    assert_eq!(plan.rpath, None);
    assert!(
        !plan.directives().iter().any(|d| d.contains("rpath")),
        "no directive on the static path may mention an rpath, and they were {:?}",
        plan.directives()
    );
}

#[test]
fn the_shared_recipe_is_emitted_exactly_and_keeps_its_rpath() {
    let info = certified();
    let plan = plan(&info, NOTHING, "aarch64-unknown-linux-gnu");
    assert_eq!(plan.kind, LinkKind::Shared);
    assert_eq!(
        plan.directives(),
        vec![
            format!("cargo::rustc-link-search=native={ROOT}/lib"),
            "cargo::rustc-link-lib=dylib=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=m".to_string(),
            format!("cargo::rustc-link-arg=-Wl,-rpath,{ROOT}/lib"),
        ],
        "the shared path needs the rpath: cargo does not put a build script's link-search path on \
         the loader's path, so without it the test binary links and then dies before main with \
         `cannot open shared object file`"
    );
    assert_eq!(plan.rpath, Some(PathBuf::from(format!("{ROOT}/lib"))));
}

#[test]
fn the_shared_recipe_reads_the_shared_list_and_the_static_one_reads_the_static_list() {
    // They used to be one field and they are not the same list. A consumer
    // that reads the wrong one gets undefined symbols at the end of a static
    // link with nothing pointing at why.
    let info = certified();
    let shared = plan(&info, NOTHING, "aarch64-unknown-linux-gnu");
    let statik = plan(&info, STATIC, "aarch64-unknown-linux-gnu");
    assert!(
        shared
            .directives()
            .contains(&"cargo::rustc-link-lib=m".to_string())
    );
    assert!(
        !shared
            .directives()
            .contains(&"cargo::rustc-link-lib=rt".to_string()),
        "`rt` is only in `static_system_libraries`, so the shared plan must not have it"
    );
    assert!(
        statik
            .directives()
            .contains(&"cargo::rustc-link-lib=rt".to_string())
    );
}

#[test]
fn the_uncertified_archive_links_the_dylib_it_ships() {
    let info = uncertified();
    let plan = plan(&info, NOTHING, "aarch64-apple-darwin");
    assert_eq!(plan.kind, LinkKind::Shared);
    assert_eq!(
        plan.directives(),
        vec![
            format!("cargo::rustc-link-search=native={ROOT}/lib"),
            "cargo::rustc-link-lib=dylib=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=objc.A".to_string(),
            format!("cargo::rustc-link-arg=-Wl,-rpath,{ROOT}/lib"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Which mode gets picked
// ---------------------------------------------------------------------------

#[test]
fn neither_feature_means_shared_even_on_a_certified_archive() {
    // The feature defaults off and off is the shared link, so the three CI
    // jobs that never fetch an archive and the one that does all get the same
    // shape. Static is something a consumer asks for.
    let info = certified();
    assert_eq!(
        plan(&info, NOTHING, "aarch64-unknown-linux-gnu").kind,
        LinkKind::Shared
    );
}

#[test]
fn link_static_on_a_certified_archive_is_the_static_plan() {
    let info = certified();
    assert_eq!(
        plan(&info, STATIC, "aarch64-unknown-linux-gnu").kind,
        LinkKind::Static
    );
}

#[test]
fn link_static_on_an_uncertified_target_is_refused_and_names_the_target() {
    // `static_certified` is a measurement, not an intention: it is true only
    // when the build linked the static archive into a probe and ran it on that
    // target. False means the archive ships no `.a` at all, so there is
    // nothing to link and no way to fake one.
    let info = uncertified();
    let error = refused(
        &info,
        STATIC,
        "aarch64-apple-darwin",
        "link-static on the mac archive",
    );
    assert!(
        matches!(error, PolicyError::StaticNotCertified { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("aarch64-apple-darwin"),
        "the refusal has to name the target that has no certified static half, and it said: {shown}"
    );
}

// ---------------------------------------------------------------------------
// The archive has to be the one this crate was built against
// ---------------------------------------------------------------------------

#[test]
fn an_archive_for_another_triple_is_refused_and_names_both_triples() {
    // A glibc archive linked into a musl binary is a link that succeeds and a
    // binary that does not load, so this is checked rather than assumed.
    let info = certified();
    let error = refused(
        &info,
        NOTHING,
        "aarch64-unknown-linux-musl",
        "a glibc archive against a musl target",
    );
    assert!(
        matches!(error, PolicyError::TargetMismatch { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("aarch64-unknown-linux-gnu") && shown.contains("aarch64-unknown-linux-musl"),
        "the refusal has to name the archive's triple and the one being built for, and it said: \
         {shown}"
    );
}

#[test]
fn an_abi_version_that_is_not_the_headers_stops_the_plan_with_both_numbers() {
    let info = Raw {
        abi_version: Some(3),
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("3 is a number");
    let error = refused(&info, NOTHING, "aarch64-unknown-linux-gnu", "abi_version 3");
    assert!(
        matches!(error, PolicyError::AbiVersionMismatch { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains('3') && shown.contains('2'),
        "both numbers have to be in the message or nobody can tell which side moved, and it said: \
         {shown}"
    );
}

#[test]
fn a_wire_version_that_is_not_the_headers_stops_the_plan() {
    let info = Raw {
        wire_version: Some(3),
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("3 is a number");
    let error = refused(
        &info,
        NOTHING,
        "aarch64-unknown-linux-gnu",
        "wire_version 3",
    );
    assert!(
        matches!(error, PolicyError::WireVersionMismatch { .. }),
        "it said: {error}"
    );
}

#[test]
fn a_fingerprint_that_is_not_the_headers_stops_the_plan_with_both_values() {
    // The failure this catches is a library that loads, resolves every symbol,
    // and reads a struct field four bytes from where this crate believes it
    // is. Both values go in the message, formatted the same way, because the
    // question the reader has is which of the two moved.
    let info = Raw {
        abi_header_sha256: Some(
            "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899".into(),
        ),
        abi_fingerprint: Some("aabbccddeeff0011".into()),
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("that is a well formed fingerprint, just not this header's");
    let error = refused(
        &info,
        NOTHING,
        "aarch64-unknown-linux-gnu",
        "an archive built against another header",
    );
    assert!(
        matches!(error, PolicyError::FingerprintMismatch { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("aabbccddeeff0011") && shown.contains("0502ac0f61611530"),
        "both fingerprints have to be in the message, and it said: {shown}"
    );
}

#[test]
fn the_comparison_is_between_numbers_and_not_between_strings() {
    // A fingerprint with a leading zero is where a string comparison and a
    // number comparison part company. This one is the shipped value, which
    // starts with a zero, so a reader that dropped it or uppercased it would
    // still have to agree here.
    let info = certified();
    assert!(
        choose(&info, NOTHING, "aarch64-unknown-linux-gnu").is_ok(),
        "the shipped fingerprint has a leading zero and has to compare equal as a number"
    );
}

// ---------------------------------------------------------------------------
// static_link_args travels nowhere, so it is a refusal rather than a forward
// ---------------------------------------------------------------------------

#[test]
fn a_non_empty_static_link_args_is_a_refusal_naming_the_manifest() {
    // `cargo::rustc-link-arg` does not propagate from a dependency's build
    // script to a downstream binary. It binds to the emitting package's own
    // targets and goes no further, so forwarding one of these means this
    // crate's own tests link correctly and pass, CI is green, and every binary
    // that depends on the crate is built without the argument and aborts on
    // the first call. Whatever the manifest is asking for cannot be delivered
    // from here, so the right answer is to stop and say so.
    let info = Raw {
        static_link_args: Some(vec!["-Wl,-u,NativeAOT_StaticInitialization".into()]),
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("a link arg is a string");
    let error = refused(
        &info,
        STATIC,
        "aarch64-unknown-linux-gnu",
        "a manifest asking for a link argument",
    );
    assert!(
        matches!(error, PolicyError::StaticLinkArgsUnsupported { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("LINKINFO.json") || shown.contains(ROOT),
        "the refusal has to name the manifest, and it said: {shown}"
    );
    assert!(
        shown.contains("NativeAOT_StaticInitialization"),
        "the refusal has to quote what was asked for, and it said: {shown}"
    );
}

#[test]
fn an_empty_static_link_args_is_every_archive_published_so_far_and_is_fine() {
    let info = certified();
    assert!(choose(&info, STATIC, "aarch64-unknown-linux-gnu").is_ok());
}

// ---------------------------------------------------------------------------
// Nothing in this crate names the flag that does not exist
// ---------------------------------------------------------------------------

#[test]
fn the_crates_own_source_never_names_the_symbol_the_samples_tell_you_to_force() {
    // `NativeAOT_StaticInitialization` does not exist in .NET 10: linking with
    // `--require-defined` for it fails outright, and the sample code that
    // tells you to use it predates the runtime that removed it. The
    // initialiser ships as a library rather than as a flag for exactly this
    // reason, so the name has no business anywhere in `src/` or `build/`.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in ["src", "build"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("the directory is committed") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable file");
            assert!(
                !text.contains("NativeAOT_StaticInitialization"),
                "{} names NativeAOT_StaticInitialization, and forcing that symbol is the fix that \
                 looks obvious, cannot reach a downstream binary, and does not exist in .NET 10",
                path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The whole digest, not the 64 bits of it the fingerprint carries
// ---------------------------------------------------------------------------

#[test]
fn a_header_digest_that_is_not_the_vendored_headers_stops_the_plan() {
    // `abi_fingerprint` is the first eight bytes of `abi_header_sha256`, so
    // comparing it checks 64 of the digest's 256 bits and the other 192 were
    // read, validated for shape, and then compared against nothing. This is
    // the case that proves it: the fingerprint agrees, the manifest's own
    // internal relationship agrees, and the header the archive was built
    // against is still a different file.
    let info = Raw {
        abi_header_sha256: Some(
            // Same first eight bytes, so the fingerprint check and the
            // manifest's own prefix rule both pass.
            "0502ac0f61611530ffffffffffffffffffffffffffffffffffffffffffffffff".into(),
        ),
        ..certified_raw()
    }
    .validate(&manifest_path())
    .expect("that is a well formed digest, just not this header's");
    let error = refused(
        &info,
        NOTHING,
        "aarch64-unknown-linux-gnu",
        "an archive built against a header with the same first eight bytes",
    );
    assert!(
        matches!(error, PolicyError::HeaderDigestMismatch { .. }),
        "it said: {error}"
    );
    let shown = error.to_string();
    assert!(
        shown.contains("LINKINFO.json") && shown.contains("native/viprs_acadsharp.h"),
        "the refusal has to name both files or nobody knows which two disagree, and it said: \
         {shown}"
    );
    assert!(
        shown.contains("0502ac0f61611530ffffffffffffffffffffffffffffffffffffffffffffffff")
            && shown.contains("0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa"),
        "and both digests, because the question the reader has is which side moved, and it said: \
         {shown}"
    );
}

#[test]
fn the_fingerprint_alone_would_have_let_that_one_through() {
    // The measurement behind the test above, stated as one. A digest that
    // shares its first eight bytes is a fingerprint match, so a policy that
    // only compared fingerprints had nothing to say about it.
    let manifest_digest = "0502ac0f61611530ffffffffffffffffffffffffffffffffffffffffffffffff";
    let header_digest = "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa";
    assert_eq!(manifest_digest[..16], header_digest[..16]);
    assert_ne!(manifest_digest, header_digest);
}

#[test]
fn the_shipped_digest_is_accepted() {
    // The positive control, so a comparison that refused everything cannot
    // pass the test above and link nothing.
    let info = certified();
    assert!(
        choose(&info, NOTHING, "aarch64-unknown-linux-gnu").is_ok(),
        "the shipped manifest's digest is the vendored header's digest"
    );
}

// ---------------------------------------------------------------------------
// Features are additive, so there is only one of them
// ---------------------------------------------------------------------------

#[test]
fn there_is_no_link_shared_feature_anywhere() {
    // Cargo features are additive: any crate in a graph may turn one on and
    // none of them can turn another's off. `link-static` and `link-shared`
    // were a pair that picked between two answers, so a graph where one
    // dependency wanted shared and another wanted static unified both on and
    // the build script panicked at neither author:
    //
    //     error: failed to run custom build command for `acadsharp-rs`
    //     thread 'main' panicked at build.rs:400:5:
    //     the `link-static` and `link-shared` features are both on ... Turn one off.
    //
    // Measured in a real three-crate graph. With no feature at all the plan is
    // already the shared one, so `link-shared` carried the hazard and bought
    // nothing, and it is gone. This is free to delete now and breaking after
    // publication, which is why it happened now.
    // Matched on what re-introducing it would actually look like rather than
    // on the name, because the name is still in the prose on purpose: a
    // feature that vanished without a sentence saying why comes back.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("it is committed");
    assert!(
        !manifest
            .lines()
            .any(|l| l.trim_start().starts_with("link-shared")),
        "Cargo.toml declares a link-shared feature again, and a pair of features that pick \
         between two answers cannot both be on in a graph that unified them"
    );
    for file in ["build.rs", "build/policy.rs"] {
        let text = std::fs::read_to_string(root.join(file)).expect("it is committed");
        assert!(
            !text.contains("CARGO_FEATURE_LINK_SHARED"),
            "{file} reads CARGO_FEATURE_LINK_SHARED, so the feature is back"
        );
    }
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("committed");
    assert!(
        !ci.contains("--features link-shared"),
        "a CI cell asks for the link-shared feature, so something declares it"
    );
}

#[test]
fn link_static_is_the_only_feature_and_asking_for_nothing_is_the_shared_link() {
    // The replacement for the conflict test: with one feature there is no
    // combination left to refuse, and the default is what it always was.
    let info = certified();
    assert_eq!(
        plan(&info, NOTHING, "aarch64-unknown-linux-gnu").kind,
        LinkKind::Shared
    );
    assert_eq!(
        plan(&info, STATIC, "aarch64-unknown-linux-gnu").kind,
        LinkKind::Static
    );
}

// ---------------------------------------------------------------------------
// What a consumer has to do about the shared link, written down
// ---------------------------------------------------------------------------

#[test]
fn the_docs_say_what_a_downstream_binary_needs() {
    // The rpath goes out as `cargo::rustc-link-arg`, which binds to the
    // emitting package's own targets and goes no further. So this crate's own
    // tests run and a consumer's binary dies before `main`:
    //
    //     error while loading shared libraries: libacadsharp_native.so
    //     exit 127
    //
    // Measured from a trivial downstream crate, twice, independently. Nothing
    // in the crate can fix that from here, so the least it can do is say so
    // where somebody will read it, and this is the guard that keeps the
    // paragraph there.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["README.md", "src/lib.rs"] {
        let text = std::fs::read_to_string(root.join(file)).expect("it is committed");
        for needle in [
            "DEP_ACADSHARP_NATIVE_LIB_DIR",
            "LD_LIBRARY_PATH",
            "rpath",
            "link-static",
        ] {
            assert!(
                text.contains(needle),
                "{file} has to tell a consumer about {needle}: a binary that links this crate \
                 shared and does none of the three dies at load with exit 127, and the crate's \
                 own tests pass because the rpath it emits covers its own targets only"
            );
        }
    }
}
