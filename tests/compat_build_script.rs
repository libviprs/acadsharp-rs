//! The real build script, handed an archive, read one directive at a time.
//!
//! `tests/compat.rs` tests the declaration and its checks as pure functions,
//! which is where almost everything about `COMPAT.toml` belongs. This file
//! covers the one property no pure test can see: **which** archives the
//! declaration is checked against, and **when** in the run that happens.
//!
//! Both of those are call-site facts, and both of them were wrong.
//!
//! # The archive the check never saw
//!
//! The check used to resolve `ACADSHARP_NATIVE_DIR` itself and give up when it
//! was unset. An archive is also resolved out of
//! `$CARGO_HOME/acadsharp-native/<artifact_version>/<platform>-<cpu>/`, and one
//! found there went through a build that checked nothing. Measured on the
//! composed tree, with a cached archive declaring `9.9.9-viprs.9` against a
//! declaration saying `3.7.1-viprs.*`: exit 0,
//! `cargo::metadata=artifact_version=9.9.9-viprs.9`, and
//! `cargo::rustc-cfg=acadsharp_linked`.
//!
//! So the check hangs off the archive, never off the variable, and
//! [`nothing_links_an_archive_the_declaration_has_not_cleared`] is the
//! regression test. It passes on this branch for a boring reason (this branch's
//! resolver has only the one route) and it is the test that fails when the real
//! resolver lands beside a check that is keyed on a variable again.
//!
//! # The directive the caller chose
//!
//! Cargo's build-script protocol is one directive per line of stdout, so a
//! newline in a path this script prints ends one directive and starts another.
//! The archive half canonicalises the root and refuses a control character in
//! it before it prints anything; the declaration check used to run first and
//! print the **raw** variable. A symlink whose own name carries a newline
//! resolves to a clean directory, so the guard passed, the build went green and
//! the second half of the split line reached the real link line:
//!
//! ```text
//! cc: error: unrecognized command-line option
//!     '--totally-bogus-flag/metadata/LINKINFO.json'
//! ```
//!
//! Measured on this branch, not inherited from a review.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `metadata/LINKINFO.json` out of the real `aarch64-unknown-linux-gnu`
/// archive, byte for byte, which is also what `tests/compat.rs` checks the
/// declaration against.
const REAL_MANIFEST: &str = include_str!("data/linkinfo/aarch64-unknown-linux-gnu.json");

/// The same manifest with both versions moved somewhere the declaration does
/// not cover.
///
/// Everything else stays exactly as the real archive says it, the target and
/// the three ABI numbers included, because those are what an archive policy
/// compares and I want this archive to be refused by the declaration rather
/// than by something else that happens to be looking.
fn refused_manifest() -> String {
    let out = REAL_MANIFEST
        .replace(
            "\"artifact_version\": \"3.7.1-viprs.1\"",
            "\"artifact_version\": \"9.9.9-viprs.9\"",
        )
        .replace(
            "\"acadsharp_version\": \"3.7.1\"",
            "\"acadsharp_version\": \"9.9.9\"",
        );
    assert!(
        out.contains("9.9.9-viprs.9") && out.contains("\"acadsharp_version\": \"9.9.9\""),
        "the fixture manifest no longer spells its two version fields the way this substitution \
         expects, so this test would be asserting about the real archive instead of a refused one"
    );
    out
}

/// A scratch directory of my own, under `OUT_DIR` so `cargo clean` takes it and
/// so nothing here writes into a shared temp directory another lane's tests are
/// also using.
///
/// Nothing is ever deleted here. Every file each test needs is written fresh on
/// every run, so a leftover is overwritten rather than read, and a test that
/// removes a directory tree is one wrong path away from removing somebody's.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("OUT_DIR"))
        .join("compat-build-script")
        .join(name);
    std::fs::create_dir_all(&dir).expect("I own this directory");
    dir
}

/// Lays out an unpacked archive at exactly `root`: a manifest, a `lib/`, and an
/// empty file for every library a manifest can name.
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
/// feature sets, and the newest is the one this run belongs to.
fn build_script() -> PathBuf {
    let out_dir = PathBuf::from(env!("OUT_DIR"));
    let build_dir = out_dir
        .parent()
        .and_then(Path::parent)
        .expect("OUT_DIR always sits two levels under the build directory");

    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(build_dir).expect("the build directory exists") {
        let path = entry.expect("a readable entry").path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("acadsharp-rs-"))
        {
            continue;
        }
        let candidate = path.join("build-script-build");
        if let Ok(meta) = std::fs::metadata(&candidate) {
            let when = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            found.push((when, candidate));
        }
    }
    found.sort();
    found.pop().map(|(_, path)| path).unwrap_or_else(|| {
        panic!(
            "I could not find a compiled build script under {}. Cargo builds one for this package \
             before it builds this test, so if there is none here then the layout of the target \
             directory has changed and this test needs to learn the new one.",
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
            .filter(|line| line.starts_with("cargo::"))
            .collect()
    }

    fn has_cfg(&self, name: &str) -> bool {
        self.directives()
            .iter()
            .any(|line| *line == format!("cargo::rustc-cfg={name}"))
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
    cargo_home: PathBuf,
}

impl Run {
    /// A run with no archive anywhere and a cargo home of its own.
    ///
    /// The empty cargo home matters: this suite is itself run with
    /// `ACADSHARP_NATIVE_DIR` set about half the time and on a machine that may
    /// have archives unpacked in the real cache, and a run that inherited
    /// either would be testing whatever the developer happened to have.
    fn new(scratch: &Path) -> Self {
        let cargo_home = scratch.join("cargo-home");
        std::fs::create_dir_all(&cargo_home).expect("I own this directory");
        Self {
            target: "aarch64-unknown-linux-gnu".into(),
            native_dir: None,
            cargo_home,
        }
    }

    /// Points `ACADSHARP_NATIVE_DIR` at a directory.
    fn native_dir(mut self, dir: PathBuf) -> Self {
        self.native_dir = Some(dir);
        self
    }

    /// Puts an archive where the cache would have it, and leaves
    /// `ACADSHARP_NATIVE_DIR` unset.
    ///
    /// The layout is the documented one,
    /// `$CARGO_HOME/acadsharp-native/<artifact_version>/<platform>-<cpu>/`.
    fn cached_archive(self, artifact_version: &str, leaf: &str, manifest_text: &str) -> Self {
        let root = self
            .cargo_home
            .join("acadsharp-native")
            .join(artifact_version)
            .join(leaf);
        unpack_at(&root, manifest_text);
        self
    }

    fn go(self, scratch: &Path) -> Outcome {
        let out_dir = scratch.join("out");
        std::fs::create_dir_all(&out_dir).expect("I own this directory");

        let mut env: BTreeMap<&str, String> = BTreeMap::new();
        env.insert("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR").to_string());
        env.insert("OUT_DIR", out_dir.display().to_string());
        env.insert("TARGET", self.target.clone());
        env.insert("CARGO_HOME", self.cargo_home.display().to_string());
        if let Some(dir) = &self.native_dir {
            env.insert("ACADSHARP_NATIVE_DIR", dir.display().to_string());
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
// The two controls
// ---------------------------------------------------------------------------

#[test]
fn the_archive_the_declaration_covers_is_linked() {
    // The positive control. Every refusal below would also pass against a
    // build script that refused everything.
    let dir = scratch("accepts-the-real-archive");
    let archive = unpack_at(&dir.join("archive"), REAL_MANIFEST);
    let outcome = Run::new(&dir).native_dir(archive).go(&dir);

    assert!(
        outcome.ok && outcome.has_cfg("acadsharp_linked"),
        "the archive that ships today is the one COMPAT.toml declares, so it has to link. The \
         build script said:\n{}",
        outcome.everything()
    );
}

#[test]
fn an_archive_the_declaration_refuses_stops_the_build_naming_compat_toml() {
    let dir = scratch("refuses-an-undeclared-archive");
    let archive = unpack_at(&dir.join("archive"), &refused_manifest());
    let outcome = Run::new(&dir).native_dir(archive).go(&dir);

    assert!(
        !outcome.ok,
        "an archive declaring 9.9.9-viprs.9 against a declaration of 3.7.1-viprs.* has to stop \
         the build, and the build script said:\n{}",
        outcome.everything()
    );
    assert!(
        outcome.stderr.contains("COMPAT.toml") && outcome.stderr.contains("9.9.9-viprs.9"),
        "the refusal has to name the file somebody edits and the version it got, and it said:\n{}",
        outcome.everything()
    );
    assert!(
        !outcome.has_cfg("acadsharp_linked"),
        "a refused archive must not have been linked on the way to being refused:\n{}",
        outcome.everything()
    );
}

// ---------------------------------------------------------------------------
// Every archive that resolves, by whichever route it resolved
// ---------------------------------------------------------------------------

/// The cache route, which is the half of the acceptance that was not covered.
///
/// `ACADSHARP_NATIVE_DIR` is unset and the archive is where the cache keeps
/// them. The declaration has to reach this archive too, because the README
/// table this crate generates says those two keys are checked "when one
/// resolves", and a cache hit is a resolution.
///
/// There are two honest outcomes and this asserts the property both share:
/// nothing links. On this branch the resolver has only the variable route, so
/// it finds nothing and links nothing. Composed with the real resolver it finds
/// this archive, and then the declaration is what stops it. What must never
/// happen is the third thing, which is what I measured on the composed tree
/// before this: `cargo::rustc-cfg=acadsharp_linked`, exit 0, and
/// `cargo::metadata=artifact_version=9.9.9-viprs.9` handed to every consumer.
#[test]
fn nothing_links_an_archive_the_declaration_has_not_cleared() {
    let dir = scratch("cache-route");
    let outcome = Run::new(&dir)
        .cached_archive("9.9.9-viprs.9", "linux-arm64", &refused_manifest())
        .go(&dir);

    assert!(
        !outcome.has_cfg("acadsharp_linked"),
        "the build script linked an archive declaring 9.9.9-viprs.9 while COMPAT.toml declares \
         3.7.1-viprs.*, because it found it somewhere the declaration check was not looking. The \
         check belongs on the archive that resolved, never on the variable one route happens to \
         read. The build script said:\n{}",
        outcome.everything()
    );
    assert!(
        !outcome
            .directives()
            .iter()
            .any(|line| line.contains("9.9.9-viprs.9")),
        "no directive may carry an archive the declaration has not cleared, and this run \
         published one:\n{}",
        outcome.everything()
    );
}

// ---------------------------------------------------------------------------
// Nothing this half prints is built from the raw variable
// ---------------------------------------------------------------------------

/// A symlink whose own name carries a newline, pointing at a clean archive.
///
/// `canonicalize` resolves the link, so the archive half's control-character
/// guard sees the clean directory and passes. Anything that printed the raw
/// variable instead ended one directive early and started a second one the
/// caller wrote.
#[cfg(unix)]
#[test]
fn no_directive_is_built_from_the_raw_variable() {
    let dir = scratch("raw-variable");
    let archive = unpack_at(&dir.join("archive"), REAL_MANIFEST);

    let link = dir.join("evil\ncargo::rustc-link-arg=--totally-bogus-flag");
    // Written fresh every run, and never removed: a test that deletes a path it
    // built out of a string is a test one bad string away from deleting
    // somebody else's.
    if std::fs::symlink_metadata(&link).is_ok() {
        std::fs::remove_file(&link).expect("I made this symlink on a previous run");
    }
    std::os::unix::fs::symlink(&archive, &link).expect("I own this directory");

    let outcome = Run::new(&dir).native_dir(link).go(&dir);

    assert!(
        !outcome.stdout.contains("totally-bogus-flag"),
        "a directive the caller chose reached cargo. The name of the directory\n  \
         ACADSHARP_NATIVE_DIR points at carried a newline, and cargo reads one directive per \
         line, so printing the raw variable ends the line the caller wanted ended. Everything \
         that carries the root has to be printed after the archive half has canonicalised it and \
         refused a control character in it. The build script said:\n{}",
        outcome.everything()
    );
}

/// The manifest is one file, so it is named once.
///
/// Two halves of the build script each reading it, each printing their own
/// `rerun-if-changed` for it, is how I noticed there were two halves reading
/// it. A duplicate is harmless to cargo and it is the symptom worth keeping a
/// test on, because the second reader is the one that had not been through the
/// root guard.
#[test]
fn the_manifest_is_named_once_in_the_rerun_lines() {
    let dir = scratch("one-rerun-line");
    let archive = unpack_at(&dir.join("archive"), REAL_MANIFEST);
    let outcome = Run::new(&dir).native_dir(archive).go(&dir);

    let named: Vec<&str> = outcome
        .directives()
        .into_iter()
        .filter(|line| line.starts_with("cargo::rerun-if-changed=") && line.contains("LINKINFO"))
        .collect();
    assert_eq!(
        named.len(),
        1,
        "`metadata/LINKINFO.json` is named {} times in the rerun lines, and it is one file read \
         by what should be one reader:\n{:#?}",
        named.len(),
        named
    );
}
