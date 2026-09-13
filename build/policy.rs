//! Which way this crate links, and the exact lines that say so.
//!
//! [`choose`] does no I/O at all. It takes a validated manifest, the two
//! feature flags, the target triple cargo is building for, the three numbers
//! the vendored header says to expect and where the archive is unpacked, and
//! hands back either a [`LinkPlan`] or a refusal. That is the whole reason it
//! is its own file: every rule in it is then a plain function call from
//! `tests/link_policy.rs`, and a rule that only existed inside `build.rs` could
//! only be tested by building something.
//!
//! # The one silent failure
//!
//! The static recipe has exactly one, and it is why the plan is a list of exact
//! strings rather than a set of flags somebody assembles later.
//!
//! Getting the order of the two `rustc-link-lib` lines wrong fails loudly on
//! `RhRegisterOSModule`, and so does leaving the default `+bundle` on either of
//! them. Omitting `+whole-archive` on the init archive links clean, resolves
//! every symbol, and aborts on the first call into the library: that archive
//! holds one object, the .NET runtime's static initialiser, it lives in
//! `.init_array`, and it defines no global symbol anything references, so
//! ordinary archive semantics never pull it in.
//!
//! And the rpath belongs to the shared plan and to nothing else. `lib/` holds
//! both `libacadsharp_native.so` and `libacadsharp_native.a`, a bare `-l` picks
//! the `.so`, so an rpath left on the static path means a binary that was
//! supposed to be self-contained links, loads and runs correctly on the build
//! machine by quietly using the shared library.

// This file and its two neighbours are compiled into two different crates: the
// build script, and the test binaries that check them. Each uses a different
// subset, so something unused here is used over there.
#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};

use crate::manifest::LinkInfo;

/// Where an archive is unpacked, and the two paths inside it anything here
/// needs to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// The directory holding `lib/`, `include/` and `metadata/`.
    pub root: PathBuf,
    /// `<root>/lib`, which is the only search path this crate ever emits.
    pub lib_dir: PathBuf,
    /// `<root>/metadata/LINKINFO.json`, which every refusal names.
    pub manifest: PathBuf,
}

impl Archive {
    /// The three paths, from the one the caller resolved.
    pub fn at(root: PathBuf) -> Self {
        let lib_dir = root.join("lib");
        let manifest = root.join("metadata").join("LINKINFO.json");
        Self {
            root,
            lib_dir,
            manifest,
        }
    }
}

/// The two link features, read from `CARGO_FEATURE_*` rather than through
/// `cfg!`.
///
/// A build script compiled with a feature and one told about it at run time
/// behave the same way, and the second kind can be driven from a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    pub link_static: bool,
    pub link_shared: bool,
}

/// The three numbers the vendored header declares, which an archive has to
/// agree with.
///
/// Passed in rather than read from a constant here, because where they come
/// from is the expectations half of `build.rs` and that half belongs to another
/// issue. This file only compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expectations {
    pub abi_version: u32,
    pub wire_version: u32,
    pub abi_fingerprint: u64,
}

/// Which of the two link modes was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Both archives, in order, with the modifiers that make that work.
    Static,
    /// The shared library plus an rpath, which is the default.
    Shared,
}

/// Everything that gets printed, in the order it gets printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPlan {
    /// Which mode this is.
    pub kind: LinkKind,
    /// The one directory that goes on the link search path.
    pub lib_dir: PathBuf,
    /// The exact values that follow `cargo::rustc-link-lib=`, in order.
    pub libraries: Vec<String>,
    /// The run path, on the shared plan and never on the static one.
    pub rpath: Option<PathBuf>,
}

impl LinkPlan {
    /// The lines to print, in order.
    pub fn directives(&self) -> Vec<String> {
        let mut out = vec![format!(
            "cargo::rustc-link-search=native={}",
            self.lib_dir.display()
        )];
        for library in &self.libraries {
            out.push(format!("cargo::rustc-link-lib={library}"));
        }
        if let Some(rpath) = &self.rpath {
            // Scoped to this package's own binaries, tests and examples.
            // `rustc-link-arg` binds to the emitting package's targets and goes
            // no further, which is a limitation everywhere else in this file
            // and is exactly right here: a consumer linking shared needs its
            // own answer, and this one is about making this crate's own tests
            // find the library they just linked.
            out.push(format!(
                "cargo::rustc-link-arg=-Wl,-rpath,{}",
                rpath.display()
            ));
        }
        out
    }
}

/// Every way the choice can fail to be one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// Both link features at once.
    FeatureConflict { manifest: String },
    /// The archive is for another target triple.
    TargetMismatch {
        manifest: String,
        archive_target: String,
        building_for: String,
    },
    /// The library was built against a different header revision.
    AbiVersionMismatch {
        manifest: String,
        found: u32,
        expected: u32,
    },
    /// The batch protocol the archive writes is not the one this crate reads.
    WireVersionMismatch {
        manifest: String,
        found: u32,
        expected: u32,
    },
    /// The fingerprints disagree, which is the one that catches a library that
    /// loads and resolves every symbol and reads a field from the wrong offset.
    FingerprintMismatch {
        manifest: String,
        found: u64,
        found_text: String,
        expected: u64,
    },
    /// `link-static` on an archive that never certified a static link.
    StaticNotCertified { manifest: String, target: String },
    /// A manifest asking for link arguments, which cannot be delivered from a
    /// dependency's build script and so is a refusal rather than a forward.
    StaticLinkArgsUnsupported { manifest: String, args: Vec<String> },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FeatureConflict { manifest } => write!(
                f,
                "the `link-static` and `link-shared` features are both on, and they pick between \
                 two different ways of linking {manifest}, so there is no answer to give. Turn \
                 one off. Both are off by default and the default is the shared link."
            ),
            Self::TargetMismatch {
                manifest,
                archive_target,
                building_for,
            } => write!(
                f,
                "{manifest} says this archive is for {archive_target} and I am building for \
                 {building_for}. These are not interchangeable: a glibc archive linked into a \
                 musl binary is a link that succeeds and a binary that does not load. Point \
                 ACADSHARP_NATIVE_DIR at the archive for the target you are building."
            ),
            Self::AbiVersionMismatch {
                manifest,
                found,
                expected,
            } => write!(
                f,
                "{manifest} says `abi_version` {found} and the header this crate vendored \
                 declares {expected}. The archive and the crate came from different revisions of \
                 the contract."
            ),
            Self::WireVersionMismatch {
                manifest,
                found,
                expected,
            } => write!(
                f,
                "{manifest} says `wire_version` {found} and the header this crate vendored \
                 declares {expected}, so the batch stream this library writes is not the one the \
                 decoder in here reads."
            ),
            Self::FingerprintMismatch {
                manifest,
                found,
                found_text,
                expected,
            } => write!(
                f,
                "{manifest} says `abi_fingerprint` {found_text} ({found:#018x}) and the header \
                 this crate vendored hashes to {expected:#018x}. That means the library and the \
                 header came from different commits, and the failure it would turn into is a \
                 library that loads, resolves every symbol, and reads a struct field four bytes \
                 from where this crate believes it is. Take the archive that matches, or \
                 re-vendor the header."
            ),
            Self::StaticNotCertified { manifest, target } => write!(
                f,
                "the `link-static` feature is on and {manifest} says `static_certified: false` \
                 for {target}, so this archive ships no static library at all and there is \
                 nothing to link. `static_certified` is a measurement: it is true only where the \
                 build linked the static archive into a probe and ran it. Use the shared link on \
                 this target, or take an archive for a target where the static link was \
                 certified."
            ),
            Self::StaticLinkArgsUnsupported { manifest, args } => write!(
                f,
                "{manifest} asks for the link arguments {args:?}, and I will not pass those on. \
                 `cargo::rustc-link-arg` does not propagate from a dependency's build script to \
                 a downstream binary: it binds to the emitting package's own targets and goes no \
                 further. So forwarding these would make this crate's own tests link correctly \
                 and pass while every binary that depends on the crate was built without them. \
                 Whatever this is asking for has to travel as a library, which is what \
                 `static_init_library` is."
            ),
        }
    }
}

/// Picks a link mode and builds the exact directive list for it.
///
/// The order of the checks is deliberate. The feature conflict comes first
/// because it is about the caller rather than the archive, then the archive is
/// checked for being the right archive at all, and only then does the mode get
/// picked.
pub fn choose(
    info: &LinkInfo,
    features: Features,
    target: &str,
    expected: Expectations,
    archive: &Archive,
) -> Result<LinkPlan, PolicyError> {
    let at = archive.manifest.display().to_string();

    if features.link_static && features.link_shared {
        return Err(PolicyError::FeatureConflict { manifest: at });
    }

    if info.target != target {
        return Err(PolicyError::TargetMismatch {
            manifest: at,
            archive_target: info.target.clone(),
            building_for: target.to_string(),
        });
    }

    if info.abi_version != expected.abi_version {
        return Err(PolicyError::AbiVersionMismatch {
            manifest: at,
            found: info.abi_version,
            expected: expected.abi_version,
        });
    }
    if info.wire_version != expected.wire_version {
        return Err(PolicyError::WireVersionMismatch {
            manifest: at,
            found: info.wire_version,
            expected: expected.wire_version,
        });
    }
    // Numbers, never strings. A leading zero, an uppercase digit or a missing
    // pad character all make two correct sides disagree over presentation.
    if info.abi_fingerprint != expected.abi_fingerprint {
        return Err(PolicyError::FingerprintMismatch {
            manifest: at,
            found: info.abi_fingerprint,
            found_text: info.abi_fingerprint_text.clone(),
            expected: expected.abi_fingerprint,
        });
    }

    // Empty in every archive published so far, and a non-empty one cannot be
    // delivered from here, so it is a refusal rather than something to forward.
    // Checked whatever mode is about to be picked: the field only exists on a
    // certified archive, and an archive that needs a flag to link is one this
    // build script cannot honour either way.
    if let Some(statics) = &info.statics
        && !statics.link_args.is_empty()
    {
        return Err(PolicyError::StaticLinkArgsUnsupported {
            manifest: at,
            args: statics.link_args.clone(),
        });
    }

    // Static only when it was asked for. Both features are off by default and
    // the default is the shared link, so the three CI jobs that never fetch an
    // archive and the one that does agree about what they are building.
    if features.link_static {
        let Some(statics) = &info.statics else {
            return Err(PolicyError::StaticNotCertified {
                manifest: at,
                target: info.target.clone(),
            });
        };

        let mut libraries = vec![
            // The init archive first, and whole. Nothing references what is in
            // it, and a linker reads left to right: with this one last the link
            // dies on `undefined reference to RhRegisterOSModule`, and without
            // `+whole-archive` the link is clean and the binary aborts at the
            // first call.
            format!(
                "static:-bundle,+whole-archive={}",
                statics.init_library_stem
            ),
            // Then the main archive, ordinary. `-bundle` on both, because with
            // the default rustc packs the objects into this crate's own rlib
            // instead of emitting `-l` flags, and that rlib lands ahead of the
            // whole-archived init archive on the final link line.
            format!("static:-bundle={}", statics.library_stem),
        ];
        libraries.extend(statics.system_libraries.iter().cloned());

        return Ok(LinkPlan {
            kind: LinkKind::Static,
            lib_dir: archive.lib_dir.clone(),
            libraries,
            // And no rpath. See this module's own documentation: an rpath here
            // is what lets a link that quietly fell back to the shared library
            // look exactly like the static one it was supposed to be.
            rpath: None,
        });
    }

    let mut libraries = vec![format!("dylib={}", info.shared_library_stem)];
    libraries.extend(info.shared_system_libraries.iter().cloned());
    Ok(LinkPlan {
        kind: LinkKind::Shared,
        lib_dir: archive.lib_dir.clone(),
        libraries,
        // Cargo does not put a build script's `rustc-link-search` path on the
        // loader's path. Measured: LD_LIBRARY_PATH in a test runner holds
        // `target/debug`, `target/debug/deps` and the toolchain's own lib
        // directories, nothing else. So without this the test binary links and
        // then dies before `main` with "cannot open shared object file", which
        // is a 127 from the loader and looks nothing like a missing search path.
        rpath: Some(archive.lib_dir.clone()),
    })
}

/// The archive-relative library paths a plan of this kind actually needs on
/// disk, so a truncated unpack is caught here rather than at link time.
pub fn required_files(info: &LinkInfo, kind: LinkKind) -> Vec<&str> {
    match kind {
        LinkKind::Shared => vec![info.shared_library.as_str()],
        LinkKind::Static => info
            .statics
            .as_ref()
            .map(|s| vec![s.init_library.as_str(), s.library.as_str()])
            .unwrap_or_default(),
    }
}

/// Where an archive for this target would sit in the cache, as a relative
/// `<platform>-<cpu>` directory name.
///
/// Only the five triples the producer publishes. Anything else has no archive
/// to find, and saying so beats guessing a directory name that will never
/// exist.
pub fn cache_leaf(target: &str) -> Option<&'static str> {
    match target {
        "x86_64-unknown-linux-gnu" => Some("linux-x64"),
        "aarch64-unknown-linux-gnu" => Some("linux-arm64"),
        "x86_64-unknown-linux-musl" => Some("musl-x64"),
        "aarch64-unknown-linux-musl" => Some("musl-arm64"),
        "aarch64-apple-darwin" => Some("mac-arm64"),
        _ => None,
    }
}

/// A small helper so `build.rs` and the tests agree about what a missing file
/// is called.
pub fn missing_file(archive: &Archive, relative: &str) -> Option<PathBuf> {
    let path = join_relative(&archive.root, relative);
    if path.is_file() { None } else { Some(path) }
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}
