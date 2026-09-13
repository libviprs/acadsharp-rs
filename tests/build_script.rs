//! The real build script, run over the two real manifests, read line by line.
//!
//! Everything else about the manifest is tested as a pure function.
//! This file is the end of the chain: it takes the compiled `build.rs`, hands
//! it an environment and an archive, and asserts on the exact directives that
//! come out of its stdout. That covers the one layer the pure tests cannot
//! reach, the `serde_json` parse in `build/linkinfo.rs`, because `serde` and
//! `serde_json` are `[build-dependencies]` and a test binary cannot link them.
//!
//! It also covers the thing worth covering most: the two manifests here are
//! byte-for-byte the ones inside the published archives, so the expectations
//! are not written by the same hand as the parser. `LINKINFO.md`'s own field
//! table says `abi_version` is "1 today" and its worked examples say `1`, while
//! every shipped archive says `2`. A fixture copied out of the prose would bake
//! that in and pass.
//!
//! Running the build script rather than calling into it is deliberate. Cargo's
//! build-script protocol is one directive per line of stdout, and the failure
//! this whole lane exists to prevent (a static link that is silently a shared
//! one, or an init archive nothing pulled in) is invisible unless you look at
//! the actual lines in their actual order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The two real manifests, vendored from the archives they shipped in.
fn fixture(target: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/linkinfo")
        .join(format!("{target}.json"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()))
}

/// A scratch directory of my own, under `OUT_DIR` so `cargo clean` takes it and
/// so nothing here writes to a shared temp directory that another lane's tests
/// are also using.
///
/// Nothing is deleted here, on purpose: every file each test needs is written
/// fresh on every run, so a leftover from a previous run is overwritten rather
/// than read, and a test that removes a directory tree is a test one wrong
/// path away from removing somebody's.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("OUT_DIR"))
        .join("build-script-tests")
        .join(name);
    std::fs::create_dir_all(&dir).expect("I own this directory");
    dir
}

/// Lays out an unpacked archive: `metadata/LINKINFO.json`, a `lib/` and an
/// empty file for every library the manifest names.
fn unpack(dir: &Path, manifest_text: &str) -> PathBuf {
    unpack_at(&dir.join("acadsharp-archive"), manifest_text)
}

/// The same, at exactly the directory given, which is what the cache layout
/// needs: there the archive root is the `<platform>-<cpu>` directory itself.
fn unpack_at(root: &Path, manifest_text: &str) -> PathBuf {
    let root = root.to_path_buf();
    std::fs::create_dir_all(root.join("lib")).expect("I own this directory");
    std::fs::create_dir_all(root.join("metadata")).expect("I own this directory");
    std::fs::write(root.join("metadata/LINKINFO.json"), manifest_text).expect("writable");
    for name in [
        "libacadsharp_native.so",
        "libacadsharp_native.dylib",
        "libacadsharp_native.a",
        "libacadsharp_native_init.a",
    ] {
        std::fs::write(root.join("lib").join(name), b"").expect("writable");
    }
    root
}

/// The compiled build script, found from `OUT_DIR`.
///
/// `OUT_DIR` for a test target is `<target>/<profile>/build/<pkg>-<hash>/out`,
/// and the build script binary lives in a sibling `<pkg>-<hash>/` directory.
/// There can be more than one when the package has been built with different
/// feature sets; they are all the same program, because this build script reads
/// its features from `CARGO_FEATURE_*` at run time rather than through `cfg!`,
/// which is also what makes a feature combination testable from here at all.
fn build_script() -> PathBuf {
    let out_dir = PathBuf::from(env!("OUT_DIR"));
    let build_dir = out_dir
        .parent()
        .and_then(Path::parent)
        .expect("OUT_DIR always sits two levels under the build directory");

    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(build_dir).expect("the build directory exists") {
        let path = entry.expect("a readable entry").path();
        let candidate = path.join("build-script-build");
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("acadsharp-rs-"))
        {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&candidate) {
            let when = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            found.push((when, candidate));
        }
    }
    found.sort();
    found.pop().map(|(_, path)| path).unwrap_or_else(|| {
        panic!(
            "I could not find a compiled build script under {}. Cargo builds one for this \
                 package before it builds this test, so if there is none here the layout of the \
                 target directory has changed and this test needs to learn the new one.",
            build_dir.display()
        )
    })
}

/// One run of the build script and everything it said.
struct Outcome {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Outcome {
    /// Only the `cargo::` lines, in the order they were printed.
    fn directives(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .filter(|l| l.starts_with("cargo::"))
            .collect()
    }

    /// Only the link directives, which is what the recipe is about.
    fn link_directives(&self) -> Vec<&str> {
        self.directives()
            .into_iter()
            .filter(|l| l.starts_with("cargo::rustc-link-"))
            .collect()
    }

    fn has_cfg(&self, name: &str) -> bool {
        self.directives()
            .iter()
            .any(|l| *l == format!("cargo::rustc-cfg={name}"))
    }

    fn warnings(&self) -> String {
        self.stdout
            .lines()
            .filter_map(|l| l.strip_prefix("cargo::warning="))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn everything(&self) -> String {
        format!("stdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
    }
}

/// How one run is set up. Every field is something cargo would set, so the
/// build script cannot tell it is being driven from a test.
struct Run {
    target: String,
    native_dir: Option<PathBuf>,
    cargo_home: Option<PathBuf>,
    link_static: bool,
    link_shared: bool,
    require_native: bool,
}

impl Run {
    fn new(scratch: &Path) -> Self {
        // A cargo home of its own by default, and an empty one, so a run that
        // is meant to find no archive cannot quietly find the developer's.
        let home = scratch.join("cargo-home");
        std::fs::create_dir_all(&home).expect("I own this directory");
        Self {
            target: "aarch64-unknown-linux-gnu".into(),
            native_dir: None,
            cargo_home: Some(home),
            link_static: false,
            link_shared: false,
            require_native: false,
        }
    }

    fn target(mut self, target: &str) -> Self {
        self.target = target.into();
        self
    }

    fn archive(mut self, root: &Path) -> Self {
        self.native_dir = Some(root.to_path_buf());
        self
    }

    fn link_static(mut self) -> Self {
        self.link_static = true;
        self
    }

    fn link_shared(mut self) -> Self {
        self.link_shared = true;
        self
    }

    fn require_native(mut self) -> Self {
        self.require_native = true;
        self
    }

    /// A cargo home of somebody else's choosing, which is what the cache-root
    /// cases below are about.
    fn cargo_home(mut self, home: &Path) -> Self {
        self.cargo_home = Some(home.to_path_buf());
        self
    }

    fn go(self, scratch: &Path) -> Outcome {
        let out_dir = scratch.join("out");
        std::fs::create_dir_all(&out_dir).expect("I own this directory");

        // An explicit environment rather than the inherited one, because this
        // test suite is itself run with ACADSHARP_NATIVE_DIR set about half the
        // time, and a run that inherited it would be testing whatever the
        // developer happened to export.
        let mut env: BTreeMap<&str, String> = BTreeMap::new();
        env.insert("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR").to_string());
        env.insert("OUT_DIR", out_dir.display().to_string());
        env.insert("TARGET", self.target.clone());
        if let Some(dir) = &self.native_dir {
            env.insert("ACADSHARP_NATIVE_DIR", dir.display().to_string());
        }
        if let Some(home) = &self.cargo_home {
            env.insert("CARGO_HOME", home.display().to_string());
        }
        if self.link_static {
            env.insert("CARGO_FEATURE_LINK_STATIC", "1".into());
        }
        if self.link_shared {
            env.insert("CARGO_FEATURE_LINK_SHARED", "1".into());
        }
        if self.require_native {
            env.insert("ACADSHARP_REQUIRE_NATIVE", "1".into());
        }

        let mut command = Command::new(build_script());
        command.env_clear();
        for (key, value) in &env {
            command.env(key, value);
        }
        let output = command.output().expect("the build script runs");
        Outcome {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// The two real manifests, end to end
// ---------------------------------------------------------------------------

#[test]
fn the_shipped_linux_manifest_produces_the_shared_recipe() {
    let dir = scratch("linux-shared");
    let root = unpack(&dir, &fixture("aarch64-unknown-linux-gnu"));
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        run.ok,
        "the build script refused a real archive: {}",
        run.everything()
    );
    let lib = root.join("lib");
    assert_eq!(
        run.link_directives(),
        vec![
            format!("cargo::rustc-link-search=native={}", lib.display()),
            "cargo::rustc-link-lib=dylib=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=m".to_string(),
            format!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib.display()),
        ],
        "{}",
        run.everything()
    );
    assert!(run.has_cfg("acadsharp_linked"), "{}", run.everything());
    assert!(
        !run.has_cfg("acadsharp_static_linked"),
        "{}",
        run.everything()
    );
}

#[test]
fn the_shipped_linux_manifest_produces_the_static_recipe_when_asked() {
    let dir = scratch("linux-static");
    let root = unpack(&dir, &fixture("aarch64-unknown-linux-gnu"));
    let run = Run::new(&dir).archive(&root).link_static().go(&dir);

    assert!(
        run.ok,
        "the build script refused a real archive: {}",
        run.everything()
    );
    let lib = root.join("lib");
    assert_eq!(
        run.link_directives(),
        vec![
            format!("cargo::rustc-link-search=native={}", lib.display()),
            "cargo::rustc-link-lib=static:-bundle,+whole-archive=acadsharp_native_init".to_string(),
            "cargo::rustc-link-lib=static:-bundle=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=m".to_string(),
        ],
        "{}",
        run.everything()
    );
    assert!(
        !run.directives().iter().any(|d| d.contains("rpath")),
        "the static path must not emit an rpath: `lib/` holds both the `.so` and the `.a`, a bare \
         `-l` picks the `.so`, and a static link that quietly fell back to shared links, loads and \
         runs correctly on the build machine. {}",
        run.everything()
    );
    assert!(
        run.has_cfg("acadsharp_static_linked"),
        "{}",
        run.everything()
    );
}

#[test]
fn the_shipped_mac_manifest_parses_and_links_the_dylib_it_ships() {
    // The uncertified one. Nothing is compiled for this target here; the build
    // script is just reading a manifest and printing lines, and that is exactly
    // the half this test is about. It is also the fixture that proves the
    // parser survives a `shared_system_libraries` list with dots in the names.
    let dir = scratch("mac-shared");
    let root = unpack(&dir, &fixture("aarch64-apple-darwin"));
    let run = Run::new(&dir)
        .target("aarch64-apple-darwin")
        .archive(&root)
        .go(&dir);

    assert!(
        run.ok,
        "the build script refused a real archive: {}",
        run.everything()
    );
    let lib = root.join("lib");
    assert_eq!(
        run.link_directives(),
        vec![
            format!("cargo::rustc-link-search=native={}", lib.display()),
            "cargo::rustc-link-lib=dylib=acadsharp_native".to_string(),
            "cargo::rustc-link-lib=icucore.A".to_string(),
            "cargo::rustc-link-lib=objc.A".to_string(),
            "cargo::rustc-link-lib=swiftCore".to_string(),
            "cargo::rustc-link-lib=swiftFoundation".to_string(),
            "cargo::rustc-link-lib=System.B".to_string(),
            format!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib.display()),
        ],
        "{}",
        run.everything()
    );
}

#[test]
fn link_static_against_the_uncertified_mac_archive_stops_the_build() {
    let dir = scratch("mac-static");
    let root = unpack(&dir, &fixture("aarch64-apple-darwin"));
    let run = Run::new(&dir)
        .target("aarch64-apple-darwin")
        .archive(&root)
        .link_static()
        .go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("aarch64-apple-darwin"),
        "the refusal has to name the target that ships no certified static half: {}",
        run.everything()
    );
}

// ---------------------------------------------------------------------------
// The features
// ---------------------------------------------------------------------------

#[test]
fn both_features_with_an_archive_present_stops_the_build() {
    let dir = scratch("both-with-archive");
    let root = unpack(&dir, &fixture("aarch64-unknown-linux-gnu"));
    let run = Run::new(&dir)
        .archive(&root)
        .link_static()
        .link_shared()
        .go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("link-static") && run.stderr.contains("link-shared"),
        "the refusal has to name both features: {}",
        run.everything()
    );
}

#[test]
fn both_features_with_no_archive_is_green() {
    // This is what `cargo doc --all-features` does in the `Docs` job, which
    // never fetches an archive. The conflict is a real refusal, and it is
    // raised only once there is an archive to link either way, so turning
    // every feature on to render the documentation cannot fail.
    let dir = scratch("both-no-archive");
    let run = Run::new(&dir).link_static().link_shared().go(&dir);

    assert!(
        run.ok,
        "`cargo doc --all-features` with no archive has to stay green: {}",
        run.everything()
    );
    assert!(!run.has_cfg("acadsharp_linked"), "{}", run.everything());
    assert!(run.link_directives().is_empty(), "{}", run.everything());
}

// ---------------------------------------------------------------------------
// Finding an archive, and saying so when there is none
// ---------------------------------------------------------------------------

#[test]
fn the_cache_location_is_resolved_when_the_variable_is_unset() {
    let dir = scratch("cache");
    let home = dir.join("cargo-home");
    let cached = home
        .join("acadsharp-native")
        .join("3.7.1-viprs.1")
        .join("linux-arm64");
    std::fs::create_dir_all(&cached).expect("I own this directory");
    let root = unpack_at(&cached, &fixture("aarch64-unknown-linux-gnu"));

    // No ACADSHARP_NATIVE_DIR at all, which is the case the cache exists for.
    let run = Run::new(&dir).go(&dir);
    assert!(run.ok, "{}", run.everything());
    assert!(
        run.has_cfg("acadsharp_linked"),
        "an archive sitting in the documented cache location has to be found: {}",
        run.everything()
    );
    assert!(
        run.link_directives()
            .iter()
            .any(|d| d.contains(&root.join("lib").display().to_string())),
        "{}",
        run.everything()
    );
}

#[test]
fn no_archive_anywhere_names_both_places_it_looked() {
    let dir = scratch("no-archive");
    let run = Run::new(&dir).go(&dir);

    assert!(
        run.ok,
        "three of the five CI jobs never link, so a missing archive is a warning rather than a \
         failure: {}",
        run.everything()
    );
    assert!(!run.has_cfg("acadsharp_linked"), "{}", run.everything());

    let said = run.warnings();
    assert!(
        said.contains("ACADSHARP_NATIVE_DIR"),
        "the message has to name the variable it read: {said}"
    );
    assert!(
        said.contains("acadsharp-native"),
        "the message has to name the cache location it looked in: {said}"
    );
    assert!(
        said.contains("libviprs-dep"),
        "the message has to say where an archive comes from: {said}"
    );
}

#[test]
fn a_job_that_says_it_requires_the_native_lane_fails_rather_than_warns() {
    // The anti-skip rule, moved as early as it will go. `Test` sets
    // ACADSHARP_REQUIRE_NATIVE at job level, and with it set a missing archive
    // is not a warning about a lane that compiled out, it is the end of the
    // build, before anything has had the chance to go green having run nothing.
    let dir = scratch("required-no-archive");
    let run = Run::new(&dir).require_native().go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("ACADSHARP_NATIVE_DIR") && run.stderr.contains("acadsharp-native"),
        "the failure has to name both places it looked: {}",
        run.everything()
    );
}

#[test]
fn an_archive_root_with_no_manifest_in_it_is_not_an_archive() {
    let dir = scratch("empty-root");
    let root = dir.join("not-an-archive");
    std::fs::create_dir_all(root.join("lib")).expect("I own this directory");
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(run.ok, "{}", run.everything());
    assert!(!run.has_cfg("acadsharp_linked"), "{}", run.everything());
    assert!(
        run.warnings().contains("LINKINFO.json"),
        "the warning has to say which piece was missing: {}",
        run.everything()
    );
}

// ---------------------------------------------------------------------------
// A manifest this crate cannot describe stops the build
// ---------------------------------------------------------------------------

/// The real manifest with one substitution, which is how every mutation below
/// stays anchored to the shipped file rather than to a hand-written copy.
fn mutated(target: &str, from: &str, to: &str) -> String {
    let text = fixture(target);
    assert!(
        text.contains(from),
        "the fixture does not contain {from:?}, so this mutation would be testing nothing"
    );
    text.replace(from, to)
}

#[test]
fn a_fingerprint_from_another_header_stops_the_build_with_both_values() {
    let dir = scratch("wrong-fingerprint");
    // Both fields move together, because the manifest's own internal check
    // (the fingerprint is the head of the digest beside it) would otherwise
    // fire first and this test would be about the wrong rule.
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa",
        "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899",
    );
    let text = text.replace("\"0502ac0f61611530\"", "\"aabbccddeeff0011\"");
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("aabbccddeeff0011") && run.stderr.contains("0502ac0f61611530"),
        "both fingerprints have to be in the message or nobody can tell which side moved: {}",
        run.everything()
    );
}

#[test]
fn a_header_digest_from_another_revision_stops_the_build_with_both_files() {
    // The 192 bits `abi_fingerprint` never covers. This digest shares its
    // first eight bytes with the real one, so the fingerprint comparison and
    // the manifest's own prefix rule both pass and the only thing left to
    // notice is the digest itself. This is the end-to-end half: the pure test
    // in `tests/link_policy.rs` proves `choose` compares them, and this one
    // proves `build.rs` hands it the vendored header's own digest rather than
    // something it made up.
    let dir = scratch("wrong-header-digest");
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa",
        "0502ac0f61611530ffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("LINKINFO.json") && run.stderr.contains("native/viprs_acadsharp.h"),
        "the refusal has to name both files: {}",
        run.everything()
    );
    assert!(
        run.stderr
            .contains("0502ac0f61611530ffffffffffffffffffffffffffffffffffffffffffffffff")
            && run
                .stderr
                .contains("0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa"),
        "and both digests: {}",
        run.everything()
    );
}

#[test]
fn a_schema_version_from_the_future_stops_the_build() {
    let dir = scratch("future-schema");
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "\"schema_version\": 1",
        "\"schema_version\": 2",
    );
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("schema_version"),
        "{}",
        run.everything()
    );
}

#[test]
fn a_missing_required_field_stops_the_build_and_names_it() {
    let dir = scratch("missing-field");
    let text = mutated("aarch64-unknown-linux-gnu", "  \"wire_version\": 2,\n", "");
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(run.stderr.contains("wire_version"), "{}", run.everything());
}

#[test]
fn an_empty_static_library_path_stops_the_build() {
    let dir = scratch("empty-static-library");
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "\"static_library\": \"lib/libacadsharp_native.a\"",
        "\"static_library\": \"\"",
    );
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("static_library"),
        "{}",
        run.everything()
    );
}

#[test]
fn a_manifest_that_is_not_json_at_all_stops_the_build() {
    // The hand-rolled reader this replaces accepted a raw newline inside a
    // string, which is invalid JSON that a real parser refuses, and that was
    // the injection. A parser is the fix, so this checks there is one.
    let dir = scratch("injected");
    let text = "{ \"shared_system_libraries\": [\"m\ncargo::rustc-link-arg=--totally-bogus\"] }\n";
    let root = unpack(&dir, text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        !run.stdout.contains("totally-bogus"),
        "nothing out of the manifest may reach stdout before it has been refused: {}",
        run.everything()
    );
}

#[test]
fn an_archive_for_another_triple_stops_the_build() {
    let dir = scratch("wrong-triple");
    let root = unpack(&dir, &fixture("aarch64-unknown-linux-gnu"));
    let run = Run::new(&dir)
        .target("aarch64-unknown-linux-musl")
        .archive(&root)
        .go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        run.stderr.contains("aarch64-unknown-linux-musl")
            && run.stderr.contains("aarch64-unknown-linux-gnu"),
        "a glibc archive linked into a musl binary is a link that succeeds and a binary that does \
         not load, so the refusal names both triples: {}",
        run.everything()
    );
}

#[test]
fn a_manifest_naming_a_library_that_is_not_in_the_archive_stops_the_build() {
    let dir = scratch("missing-library");
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "\"shared_library\": \"lib/libacadsharp_native.so\"",
        "\"shared_library\": \"lib/libnowhere.so\"",
    );
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(run.stderr.contains("libnowhere.so"), "{}", run.everything());
}

// ---------------------------------------------------------------------------
// Nothing reaches the network
// ---------------------------------------------------------------------------

#[test]
fn nothing_in_the_build_half_of_this_crate_can_fetch_anything() {
    // The build script resolves an archive that is already on disk or it says
    // it found none. There is no download, and the way to keep it that way is
    // to notice the day somebody adds one.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = vec![root.join("build.rs")];
    for entry in std::fs::read_dir(root.join("build")).expect("the directory is committed") {
        files.push(entry.expect("a readable entry").path());
    }
    for path in files {
        let text = std::fs::read_to_string(&path).expect("a readable file");
        for forbidden in [
            "https://",
            "http://",
            "TcpStream",
            "reqwest",
            "ureq",
            "curl",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} mentions {forbidden}, and the build half of this crate never reaches the \
                 network. An archive is on disk or it is not.",
                path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Nothing out of a tarball, a symlink name or an environment variable becomes
// a directive of its own
// ---------------------------------------------------------------------------

/// Every `cargo::` line whose key is one this build script never prints.
///
/// That is the shape of the whole class: one directive splits into two and the
/// second one is somebody else's. Looking for the injected key rather than for
/// a specific string means a new way of spelling the same trick still fails
/// this.
fn foreign_directives(run: &Outcome) -> Vec<&str> {
    run.stdout
        .lines()
        .filter(|l| {
            l.starts_with("cargo::rustc-env=")
                || l.starts_with("cargo::rustc-cdylib-link-arg=")
                || l.contains("totally-bogus")
                || l.contains("PWNED")
        })
        .collect()
}

#[test]
fn an_artifact_version_carrying_a_cargo_directive_stops_the_build() {
    // Measured before the check existed. `build.rs` prints
    // `cargo::metadata=artifact_version={}`, so a newline in that string ends
    // the metadata line and starts one the tarball wrote, and cargo obeys it.
    // The library-name check next door was thorough and never looked at this
    // field.
    let dir = scratch("poisoned-artifact-version");
    let text = mutated(
        "aarch64-unknown-linux-gnu",
        "\"artifact_version\": \"3.7.1-viprs.1\"",
        "\"artifact_version\": \"3.7.1-viprs.1\\ncargo::rustc-env=PWNED=yes\"",
    );
    let root = unpack(&dir, &text);
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        foreign_directives(&run).is_empty(),
        "a directive out of the manifest reached cargo: {:?} in {}",
        foreign_directives(&run),
        run.everything()
    );
    assert!(
        run.stderr.contains("artifact_version"),
        "the refusal has to name the field: {}",
        run.everything()
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_archive_root_whose_own_name_carries_a_newline_stops_the_build() {
    // The root check used to run on `root.canonicalize().unwrap_or(root)`,
    // and canonicalising is exactly what resolves a symlink away. So a link
    // whose own name held a newline pointed at a perfectly clean directory,
    // passed the check, and then the warning printed the raw variable:
    //
    //     cargo::warning=... ACADSHARP_NATIVE_DIR, which is /tmp/i3/link
    //     cargo::rustc-link-arg=--totally-bogus, and the cache at ...
    //
    // Measured, exit 0.
    let dir = scratch("symlinked-root");
    let clean = dir.join("clean-archive-root");
    std::fs::create_dir_all(&clean).expect("I own this directory");
    let link = dir.join("link\ncargo::rustc-link-arg=--totally-bogus");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&clean, &link).expect("I own this directory");

    let run = Run::new(&dir).archive(&link).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        foreign_directives(&run).is_empty(),
        "a directive out of the variable reached cargo: {:?} in {}",
        foreign_directives(&run),
        run.everything()
    );
}

#[cfg(unix)]
#[test]
fn an_archive_root_that_carries_a_newline_and_resolves_to_nothing_stops_the_build() {
    // The half that already worked, kept as the control: with nothing to
    // canonicalise the raw path stays raw and the check sees it. Both arms
    // have to refuse, or the fix is only about symlinks.
    let dir = scratch("newline-root");
    let root = dir.join("nowhere\ncargo::rustc-link-arg=--totally-bogus");
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        foreign_directives(&run).is_empty(),
        "{:?} in {}",
        foreign_directives(&run),
        run.everything()
    );
}

#[cfg(unix)]
#[test]
fn a_cargo_home_carrying_a_cargo_directive_stops_the_build() {
    // The other route, and the one nothing checked at all. With
    // `ACADSHARP_NATIVE_DIR` unset the build script prints the cache location
    // into the same warning, and that path is `$CARGO_HOME/acadsharp-native`.
    let dir = scratch("poisoned-cargo-home");
    let home = dir.join("home\ncargo::rustc-link-arg=--totally-bogus-cache");
    std::fs::create_dir_all(&home).expect("I own this directory");
    let run = Run::new(&dir).cargo_home(&home).go(&dir);

    assert!(
        !run.ok,
        "this is supposed to stop the build: {}",
        run.everything()
    );
    assert!(
        foreign_directives(&run).is_empty(),
        "a directive out of CARGO_HOME reached cargo: {:?} in {}",
        foreign_directives(&run),
        run.everything()
    );
}

#[test]
fn an_ordinary_archive_root_with_a_space_in_it_still_works() {
    // The positive control for all four above. A path with a space in it
    // survives one directive line, and a guard that refused it would break
    // somebody's checkout for nothing.
    let dir = scratch("spaced root");
    let root = unpack_at(&dir.join("My Archives/acadsharp-linux-arm64"), &fixture("aarch64-unknown-linux-gnu"));
    let run = Run::new(&dir).archive(&root).go(&dir);

    assert!(run.ok, "{}", run.everything());
    assert!(run.has_cfg("acadsharp_linked"), "{}", run.everything());
}
