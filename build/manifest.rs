//! What `metadata/LINKINFO.json` means, with no JSON parser anywhere in sight.
//!
//! The archive's third frozen contract says which library to link, in what
//! order, with which system libraries, and whether this particular archive even
//! ships a static half. This module holds all of that as data plus the rules
//! that decide whether a given manifest is one: which fields have to be there,
//! what a value is allowed to look like, and which combinations of the four
//! static fields describe an archive rather than a mistake.
//!
//! The JSON layer is `linkinfo.rs` next door, on `serde_json`. Splitting them
//! is what lets everything below be tested as a plain function from
//! `tests/build_manifest.rs`, because `serde` and `serde_json` are
//! `[build-dependencies]` and a test binary cannot link those. It also keeps
//! the two jobs apart in the reading: `linkinfo.rs` turns text into fields and
//! this file decides whether those fields are a manifest.
//!
//! # Everything in here ends up in a cargo directive, so everything in here is
//! checked
//!
//! The manifest arrives inside a downloaded tarball, and cargo's build-script
//! protocol is one directive per line of stdout. So a newline inside a string
//! this reader repeats is not a broken name, it is the end of one directive and
//! the start of another one the tarball chose, and `rustc-link-arg` is
//! arbitrary linker flags. That was proven end to end against the hand-rolled
//! reader this replaces: a `shared_system_libraries` entry of
//! `"m\ncargo::rustc-link-arg=..."` put an arbitrary flag on the real link
//! line, and a newline in `ACADSHARP_NATIVE_DIR` split the `cargo::warning=`
//! line the same way.
//!
//! CI pins the archive by sha256 and that is the real defence. A developer
//! pointing at an archive they fetched by hand has no such pin, so nothing gets
//! out of here without being checked first: by the time you are holding a
//! [`LinkInfo`] every string in it is fit to print.

// This file and its two neighbours are compiled into two different crates: the
// build script, and the test binaries that check them. Each uses a different
// subset, so something unused here is used over there.
#![allow(dead_code)]

use std::fmt;
use std::path::Path;

/// The manifest shape this crate implements.
///
/// A manifest saying anything higher is refused outright. A higher number means
/// a field being read may no longer mean what it meant, and the failures that
/// come of guessing here are link-time or run-time crashes in a downstream
/// binary, a long way from the manifest that caused them.
pub const KNOWN_SCHEMA_VERSION: u64 = 1;

/// Every key this reader knows about, which is every key the archive ships.
///
/// `tests/build_manifest.rs` compares this against the keys in both vendored
/// manifests, in both directions, so a field the archive adds and a field this
/// reader invented are both caught.
pub const KNOWN_FIELDS: &[&str] = &[
    "schema_version",
    "artifact_version",
    "acadsharp_version",
    "acadsharp_commit",
    "dotnet_sdk",
    "target",
    "platform",
    "cpu",
    "abi_version",
    "wire_version",
    "abi_header_sha256",
    "abi_fingerprint",
    "shared_library",
    "shared_system_libraries",
    "static_library",
    "static_init_library",
    "static_certified",
    "static_system_libraries",
    "static_link_args",
    "dwg_version_min",
    "dwg_version_max",
];

/// A manifest that has been read and found to be one.
///
/// Every string in here is fit to put straight into a cargo directive and every
/// number is in range for the field it describes. The static half is an
/// `Option` rather than a `bool` beside four more `Option`s, because the four
/// static fields move together: all four present with `static_certified: true`,
/// or all four absent with `static_certified: false`, and there is no third
/// state. Making that an `Option<StaticLink>` means nothing downstream can
/// reach for a static path that was never certified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInfo {
    /// The shape of the manifest itself, never the library's version.
    pub schema_version: u64,
    /// `<upstream>-viprs.<revision>`, which is what the cache path is keyed on.
    pub artifact_version: String,
    /// The upstream library version alone. Provenance.
    pub acadsharp_version: String,
    /// The upstream commit the source tarball came from. Provenance.
    pub acadsharp_commit: String,
    /// The SDK that published the binaries. Provenance.
    pub dotnet_sdk: String,
    /// The Rust target triple this archive is for.
    pub target: String,
    /// `linux`, `musl` or `mac`, this repo's own vocabulary for `target`.
    pub platform: String,
    /// `x64` or `arm64`.
    pub cpu: String,
    /// `VIPRS_ACAD_ABI_VERSION` as the shipped header defines it.
    pub abi_version: u32,
    /// `VIPRS_ACAD_WIRE_VERSION` as the shipped header defines it.
    pub wire_version: u32,
    /// The lowest DWG format this build reads.
    pub dwg_version_min: u32,
    /// The highest, never below the lowest.
    pub dwg_version_max: u32,
    /// The sha256 of the header shipped in this same archive.
    pub abi_header_sha256: String,
    /// The fingerprint as the manifest wrote it, kept for messages only.
    pub abi_fingerprint_text: String,
    /// The same value as the number everything actually compares.
    ///
    /// It is a string in the JSON because 64 unsigned bits do not survive every
    /// JSON number reader intact, not because it is text. Comparing the text
    /// instead makes the check depend on whether somebody's formatter emitted
    /// uppercase and whether it padded to 16 characters.
    pub abi_fingerprint: u64,
    /// Archive-relative path to the shared library.
    pub shared_library: String,
    /// The bare `-l` name that path comes down to.
    pub shared_library_stem: String,
    /// Bare library names the shared library needs at load time.
    pub shared_system_libraries: Vec<String>,
    /// The static half, or nothing at all when it was never certified.
    pub statics: Option<StaticLink>,
}

impl LinkInfo {
    /// Whether the static archive was linked and run on this target during the
    /// build, which is the only thing `static_certified` ever means.
    pub fn static_certified(&self) -> bool {
        self.statics.is_some()
    }
}

/// The four fields that only exist on a certified archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticLink {
    /// Archive-relative path to the merged static archive.
    pub library: String,
    /// Its bare `-l` name.
    pub library_stem: String,
    /// Archive-relative path to the runtime's static initialiser, on its own.
    pub init_library: String,
    /// Its bare `-l` name.
    pub init_library_stem: String,
    /// Bare library names the certified static link needed, measured from the
    /// link that worked rather than assumed.
    pub system_libraries: Vec<String>,
    /// Extra flags that link needed. Empty in every archive published so far,
    /// and a non-empty one is a refusal rather than something to forward.
    pub link_args: Vec<String>,
}

/// A manifest, as read, before anything has decided whether it is one.
///
/// Every field is an `Option` because absent is a fact this reader has to be
/// able to see. An empty string is a path, an empty array is a measurement of
/// none, and neither of those means "not measured", so the shape tests for the
/// key rather than for truthiness of the value.
#[derive(Debug, Default, Clone)]
pub struct Raw {
    pub schema_version: Option<u64>,
    pub artifact_version: Option<String>,
    pub acadsharp_version: Option<String>,
    pub acadsharp_commit: Option<String>,
    pub dotnet_sdk: Option<String>,
    pub target: Option<String>,
    pub platform: Option<String>,
    pub cpu: Option<String>,
    pub abi_version: Option<u64>,
    pub wire_version: Option<u64>,
    pub abi_header_sha256: Option<String>,
    pub abi_fingerprint: Option<String>,
    pub shared_library: Option<String>,
    pub shared_system_libraries: Option<Vec<String>>,
    pub static_library: Option<String>,
    pub static_init_library: Option<String>,
    pub static_certified: Option<bool>,
    pub static_system_libraries: Option<Vec<String>>,
    pub static_link_args: Option<Vec<String>>,
    pub dwg_version_min: Option<u64>,
    pub dwg_version_max: Option<u64>,
}

/// Every way a manifest can fail to be one.
///
/// One variant per reason rather than a string, so a test can name the rule it
/// is checking and a message can be written once. The manifest path rides along
/// in every one of them: a build script's refusal is read by somebody who has
/// no idea which archive is on their machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkInfoError {
    /// The file could not be read at all.
    Unreadable { manifest: String, why: String },
    /// It is not JSON, or not JSON of the shape this reader expects.
    Json { manifest: String, why: String },
    /// A field that is present in every archive on every target is missing.
    MissingField {
        manifest: String,
        field: &'static str,
    },
    /// The manifest describes a shape this crate cannot describe.
    SchemaVersionTooNew {
        manifest: String,
        found: u64,
        known: u64,
    },
    /// `static_certified: true` with one of the four static fields absent.
    StaticFieldMissing {
        manifest: String,
        field: &'static str,
    },
    /// A static field beside `static_certified: false`, which is exactly the
    /// shape a build script author reads as belonging to the shared link.
    StaticFieldUnexpected {
        manifest: String,
        field: &'static str,
    },
    /// A present-and-empty string where absent was the only other option.
    EmptyField {
        manifest: String,
        field: &'static str,
    },
    /// A control character in one of the plain string fields, which splits a
    /// cargo directive exactly the way one in a library name does.
    Unprintable {
        manifest: String,
        field: &'static str,
        value: String,
        bad: char,
    },
    /// `abi_fingerprint` is not 16 lowercase hex characters with no prefix.
    Fingerprint {
        manifest: String,
        value: String,
        why: &'static str,
    },
    /// `abi_fingerprint` is well formed and is not the head of the digest
    /// sitting beside it, which is the relationship a consumer checks.
    FingerprintDoesNotMatchDigest {
        manifest: String,
        fingerprint: String,
        digest: String,
    },
    /// `abi_header_sha256` is not 64 lowercase hex characters.
    HeaderDigest { manifest: String, value: String },
    /// A library name with something in it that a bare library name may not
    /// contain, which is what makes this a security check and not a typo check.
    LibraryName {
        manifest: String,
        field: &'static str,
        name: String,
        bad: char,
    },
    /// A library path that is not an archive-relative path to a library.
    LibraryPath {
        manifest: String,
        field: &'static str,
        path: String,
        why: &'static str,
    },
    /// A number that does not fit the field it describes.
    NumberOutOfRange {
        manifest: String,
        field: &'static str,
        value: u64,
    },
    /// `dwg_version_max` below `dwg_version_min`.
    DwgRange {
        manifest: String,
        min: u64,
        max: u64,
    },
    /// A path with a control character in it, which splits a cargo directive.
    ArchiveRoot {
        root: String,
        manifest: String,
        bad: char,
    },
    /// The same thing one level up, for the directory the cache lives under.
    CacheRoot { root: String, bad: char },
}

impl fmt::Display for LinkInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { manifest, why } => {
                write!(f, "I could not read {manifest}: {why}")
            }
            Self::Json { manifest, why } => write!(
                f,
                "{manifest} is not a manifest I can read: {why}. It is meant to be one flat JSON \
                 object, UTF-8, no comments and no nesting beyond arrays of strings."
            ),
            Self::MissingField { manifest, field } => write!(
                f,
                "{manifest} has no `{field}` field. That one is present in every archive this \
                 producer publishes, on every target, so its absence is a malformed manifest \
                 rather than a fact about the build."
            ),
            Self::SchemaVersionTooNew {
                manifest,
                found,
                known,
            } => write!(
                f,
                "{manifest} says `schema_version` {found} and I implement {known}. An archive \
                 from the future is not a degraded archive, it is one I cannot describe: a field \
                 I am reading may no longer mean what it meant, and I would find that out as a \
                 crash in somebody's binary a long way from here. Take a newer version of this \
                 crate."
            ),
            Self::StaticFieldMissing { manifest, field } => write!(
                f,
                "{manifest} says `static_certified: true` and has no `{field}`. The four static \
                 fields move together: all four present when the static archive was linked and \
                 run, or all four absent. There is no third state."
            ),
            Self::StaticFieldUnexpected { manifest, field } => write!(
                f,
                "{manifest} says `static_certified: false` and carries a `{field}` anyway. That \
                 is exactly the shape somebody reads as belonging to the shared link. An archive \
                 that never certified a static link ships no static fields at all."
            ),
            Self::EmptyField { manifest, field } => write!(
                f,
                "{manifest} carries `\"{field}\": \"\"`. Absent is not empty: an empty string is \
                 a path nobody wrote, and a field that was not measured is left out."
            ),
            Self::Unprintable {
                manifest,
                field,
                value,
                bad,
            } => write!(
                f,
                "`{field}` in {manifest} is {value:?}, and {bad:?} is a control character I will \
                 not repeat to cargo. The build-script protocol is one directive per line of \
                 stdout, so a newline in a value I print is not a broken string: it is the end of \
                 one directive and the start of another one this manifest chose. \
                 `artifact_version` is the live one, because it goes out as \
                 `cargo::metadata=artifact_version=`, and a version pin does not save you either \
                 (a `3.7.1-viprs.*` glob matches a newline quite happily). Fix the manifest, or \
                 the archive it came out of."
            ),
            Self::Fingerprint {
                manifest,
                value,
                why,
            } => write!(
                f,
                "`abi_fingerprint` in {manifest} is {value:?}, and {why}. It is 16 lowercase \
                 hexadecimal characters with no `0x`, no separators and no sign, because it means \
                 a 64-bit number and I parse it into one before comparing."
            ),
            Self::FingerprintDoesNotMatchDigest {
                manifest,
                fingerprint,
                digest,
            } => write!(
                f,
                "`abi_fingerprint` in {manifest} is {fingerprint:?} and `abi_header_sha256` is \
                 {digest:?}. The fingerprint is defined as the first eight bytes of that digest \
                 read big-endian, so these two disagreeing means one of them was written by hand."
            ),
            Self::HeaderDigest { manifest, value } => write!(
                f,
                "`abi_header_sha256` in {manifest} is {value:?}, and a sha256 here is 64 \
                 lowercase hexadecimal characters with no prefix."
            ),
            Self::LibraryName {
                manifest,
                field,
                name,
                bad,
            } => write!(
                f,
                "`{field}` in {manifest} lists {name:?}, and {bad:?} is not something a bare \
                 library name may contain. I emit these straight after \
                 `cargo::rustc-link-lib=`, one directive per line, so a newline in there is not a \
                 broken name: it is a second directive this manifest chose and I would be \
                 repeating to cargo. Fix the manifest, or the archive it came out of."
            ),
            Self::LibraryPath {
                manifest,
                field,
                path,
                why,
            } => write!(
                f,
                "`{field}` in {manifest} is {path:?}, and {why}. These are archive-relative paths \
                 with `/` separators, and the file name is the `lib<name>.<extension>` a `-l` \
                 flag comes down to."
            ),
            Self::NumberOutOfRange {
                manifest,
                field,
                value,
            } => write!(
                f,
                "`{field}` in {manifest} is {value}, which does not fit the field the header \
                 carries it in."
            ),
            Self::DwgRange { manifest, min, max } => write!(
                f,
                "{manifest} says the DWG range runs from {min} to {max}, and the high end is \
                 never below the low one."
            ),
            Self::ArchiveRoot {
                root,
                manifest,
                bad,
            } => write!(
                f,
                "the archive root is {root:?}, and {bad:?} in a path is a control character I \
                 will not put into a cargo directive: one newline in there turns one directive \
                 into two. The manifest I would have read is {manifest:?}. Point \
                 ACADSHARP_NATIVE_DIR at a directory whose name is a directory name."
            ),
            Self::CacheRoot { root, bad } => write!(
                f,
                "the archive cache would be at {root:?}, and {bad:?} in a path is a control \
                 character I will not put into a cargo directive: one newline in there turns one \
                 directive into two. That path is CARGO_HOME, or HOME with `.cargo` on the end \
                 when CARGO_HOME is unset, so one of those two is a directory name with a control \
                 character in it."
            ),
        }
    }
}

impl Raw {
    /// Decides whether these fields are a manifest, and hands back one if so.
    ///
    /// The order of the checks is deliberate. `schema_version` comes first,
    /// because a manifest from the future is one where every check after it is
    /// asking a question about a field whose meaning may have moved.
    pub fn validate(self, manifest: &Path) -> Result<LinkInfo, LinkInfoError> {
        let at = manifest.display().to_string();

        let schema_version = required(self.schema_version, "schema_version", &at)?;
        if schema_version > KNOWN_SCHEMA_VERSION {
            return Err(LinkInfoError::SchemaVersionTooNew {
                manifest: at,
                found: schema_version,
                known: KNOWN_SCHEMA_VERSION,
            });
        }

        // `printable_text` rather than `required_text` for all seven. These are
        // the fields nothing used to look at, and `artifact_version` reaches
        // cargo directly through `cargo::metadata=artifact_version=`. The rest
        // land in refusal messages, which is a terminal rather than a
        // directive, but a rule with an exception in it is a rule somebody has
        // to remember, and these seven have no legitimate reason to hold a
        // control character.
        let artifact_version = printable_text(self.artifact_version, "artifact_version", &at)?;
        let acadsharp_version = printable_text(self.acadsharp_version, "acadsharp_version", &at)?;
        let acadsharp_commit = printable_text(self.acadsharp_commit, "acadsharp_commit", &at)?;
        let dotnet_sdk = printable_text(self.dotnet_sdk, "dotnet_sdk", &at)?;
        let target = printable_text(self.target, "target", &at)?;
        let platform = printable_text(self.platform, "platform", &at)?;
        let cpu = printable_text(self.cpu, "cpu", &at)?;

        let abi_version = required_u32(self.abi_version, "abi_version", &at)?;
        let wire_version = required_u32(self.wire_version, "wire_version", &at)?;
        let dwg_version_min = required_u32(self.dwg_version_min, "dwg_version_min", &at)?;
        let dwg_version_max = required_u32(self.dwg_version_max, "dwg_version_max", &at)?;
        if dwg_version_max < dwg_version_min {
            return Err(LinkInfoError::DwgRange {
                manifest: at,
                min: u64::from(dwg_version_min),
                max: u64::from(dwg_version_max),
            });
        }

        // `required` rather than `required_text` for these two, so an empty
        // one is refused as a malformed digest or a malformed fingerprint
        // rather than as an empty string. The shape is the interesting part.
        let abi_header_sha256 = required(self.abi_header_sha256, "abi_header_sha256", &at)?;
        check_digest(&abi_header_sha256, &at)?;
        let abi_fingerprint_text = required(self.abi_fingerprint, "abi_fingerprint", &at)?;
        let abi_fingerprint = parse_fingerprint(&abi_fingerprint_text, &abi_header_sha256, &at)?;

        let shared_library = required_text(self.shared_library, "shared_library", &at)?;
        let shared_library_stem = bare_library_name(&shared_library, "shared_library", &at)?;
        let shared_system_libraries =
            required(self.shared_system_libraries, "shared_system_libraries", &at)?;
        check_library_names(&shared_system_libraries, "shared_system_libraries", &at)?;

        // The four static fields, and the rule that they move together. Each
        // one is looked at by name so the refusal can say which one it was.
        let certified = required(self.static_certified, "static_certified", &at)?;
        let statics = if certified {
            let library = static_text(self.static_library, "static_library", &at)?;
            let library_stem = bare_library_name(&library, "static_library", &at)?;
            let init_library = static_text(self.static_init_library, "static_init_library", &at)?;
            let init_library_stem = bare_library_name(&init_library, "static_init_library", &at)?;
            let system_libraries =
                static_present(self.static_system_libraries, "static_system_libraries", &at)?;
            check_library_names(&system_libraries, "static_system_libraries", &at)?;
            let link_args = static_present(self.static_link_args, "static_link_args", &at)?;
            Some(StaticLink {
                library,
                library_stem,
                init_library,
                init_library_stem,
                system_libraries,
                link_args,
            })
        } else {
            refuse_if_present(self.static_library.is_some(), "static_library", &at)?;
            refuse_if_present(
                self.static_init_library.is_some(),
                "static_init_library",
                &at,
            )?;
            refuse_if_present(
                self.static_system_libraries.is_some(),
                "static_system_libraries",
                &at,
            )?;
            refuse_if_present(self.static_link_args.is_some(), "static_link_args", &at)?;
            None
        };

        Ok(LinkInfo {
            schema_version,
            artifact_version,
            acadsharp_version,
            acadsharp_commit,
            dotnet_sdk,
            target,
            platform,
            cpu,
            abi_version,
            wire_version,
            dwg_version_min,
            dwg_version_max,
            abi_header_sha256,
            abi_fingerprint_text,
            abi_fingerprint,
            shared_library,
            shared_library_stem,
            shared_system_libraries,
            statics,
        })
    }
}

/// Refuses an archive root with a control character in it.
///
/// Same reasoning as the library names, one layer out. The root reaches cargo
/// through `rustc-link-search`, through the rpath and through the
/// `cargo::warning=` line that says no archive resolved, and a newline in
/// `ACADSHARP_NATIVE_DIR` splits any of them. Everything else a path may
/// contain is fine: spaces, unicode, a `:`, all of it survives one directive.
///
/// This runs before the build script prints anything at all that carries the
/// root, the warning included, because the warning is one of the lines the
/// split lands in.
///
/// It runs on the **raw** variable as well as on the canonical path, and the
/// raw one first. Canonicalising is what resolves a symlink away, so a link
/// whose own name carried a newline used to point at a perfectly clean
/// directory, pass this check on the clean name, and then land raw in
/// `where_i_looked`'s warning. Measured, exit 0, with a
/// `cargo::rustc-link-arg=` of the caller's choosing on the second line.
pub fn check_archive_root(root: &Path, manifest: &Path) -> Result<(), LinkInfoError> {
    let shown = root.to_string_lossy();
    match shown.chars().find(|c| c.is_control()) {
        None => Ok(()),
        Some(bad) => Err(LinkInfoError::ArchiveRoot {
            root: shown.into_owned(),
            manifest: manifest.to_string_lossy().into_owned(),
            bad,
        }),
    }
}

/// The same rule for the cache root, which nothing checked at all.
///
/// `$CARGO_HOME/acadsharp-native` goes into the same `cargo::warning=` line
/// through `where_i_looked`, and that line is printed on the path where no
/// archive resolved, which is the path a developer with no archive is on every
/// time. There is no manifest to name here: this fires before anything has
/// looked inside the cache at all.
pub fn check_cache_root(root: &Path) -> Result<(), LinkInfoError> {
    let shown = root.to_string_lossy();
    match shown.chars().find(|c| c.is_control()) {
        None => Ok(()),
        Some(bad) => Err(LinkInfoError::CacheRoot {
            root: shown.into_owned(),
            bad,
        }),
    }
}

/// What a bare library name may be made of, and nothing else.
///
/// This is `[A-Za-z0-9_+.-]`, which covers every name any published archive has
/// listed (`m` on Linux, and `icucore.A`, `objc.A`, `swiftCore`,
/// `swiftFoundation` and `System.B` on macOS) and every plausible one:
/// `stdc++`, `pthread`, `gcc_s`, `c++abi`, `dl`, `rt`. It is deliberately an
/// allow list. A deny list of the characters I can think of today is a deny
/// list that meets a character I did not think of, and this string goes
/// straight into cargo's line-oriented protocol.
fn is_legal_in_a_library_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '.' | '-')
}

fn check_library_names(
    names: &[String],
    field: &'static str,
    at: &str,
) -> Result<(), LinkInfoError> {
    for name in names {
        // An empty name is refused first, because `find` over an empty string
        // finds nothing and an empty `-l` is a directive with no argument.
        if name.is_empty() {
            return Err(LinkInfoError::LibraryName {
                manifest: at.to_string(),
                field,
                name: name.clone(),
                bad: '\0',
            });
        }
        if let Some(bad) = name.chars().find(|c| !is_legal_in_a_library_name(*c)) {
            return Err(LinkInfoError::LibraryName {
                manifest: at.to_string(),
                field,
                name: name.clone(),
                bad,
            });
        }
    }
    Ok(())
}

/// The bare `-l` name an archive-relative library path comes down to.
///
/// `lib/libacadsharp_native_init.a` becomes `acadsharp_native_init`, with the
/// `lib` prefix and the extension stripped, which is the same transformation
/// `-l` has always done. Derived from the manifest rather than from a constant
/// in this crate, so an archive that renames its libraries keeps working.
fn bare_library_name(path: &str, field: &'static str, at: &str) -> Result<String, LinkInfoError> {
    let refuse = |why: &'static str| {
        Err(LinkInfoError::LibraryPath {
            manifest: at.to_string(),
            field,
            path: path.to_string(),
            why,
        })
    };

    if path.starts_with('/') {
        return refuse("it is absolute, and these are relative to the unpacked archive root");
    }
    if path.chars().any(char::is_control) {
        return refuse("it holds a control character, which would split a cargo directive in two");
    }
    let mut segments = path.split('/').peekable();
    let mut file = None;
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return refuse("it has an empty path segment");
        }
        if segment == "." || segment == ".." {
            return refuse("it walks out of the archive with a `.` or `..` segment");
        }
        if segments.peek().is_none() {
            file = Some(segment);
        }
    }
    let Some(file) = file else {
        return refuse("it names no file");
    };

    let Some(rest) = file.strip_prefix("lib") else {
        return refuse("the file name does not start with `lib`");
    };
    let Some((stem, extension)) = rest.rsplit_once('.') else {
        return refuse("the file name has no extension");
    };
    if !matches!(extension, "a" | "so" | "dylib") {
        return refuse("the extension is not one of `a`, `so` or `dylib`");
    }
    if stem.is_empty() {
        return refuse("there is no name left once `lib` and the extension come off");
    }
    if let Some(bad) = stem.chars().find(|c| !is_legal_in_a_library_name(*c)) {
        return Err(LinkInfoError::LibraryName {
            manifest: at.to_string(),
            field,
            name: stem.to_string(),
            bad,
        });
    }
    Ok(stem.to_string())
}

/// 16 lowercase hex characters, parsed into the number they mean.
///
/// Two mistakes are easy here and neither shows up on the happy path. A `0x`
/// prefix fails a plain base-16 parse, because most parsers reject the prefix
/// rather than skipping it, so the prefix is refused by the character check
/// before the parse ever sees it. And comparing the text to a formatted version
/// of the runtime value makes the check depend on whether the formatter emitted
/// uppercase and whether it padded a leading zero, which is why this hands back
/// a number and nothing else compares strings.
fn parse_fingerprint(value: &str, digest: &str, at: &str) -> Result<u64, LinkInfoError> {
    let refuse = |why: &'static str| {
        Err(LinkInfoError::Fingerprint {
            manifest: at.to_string(),
            value: value.to_string(),
            why,
        })
    };
    if value.len() != 16 {
        return refuse("it is not 16 characters long");
    }
    if !value.chars().all(is_lowercase_hex) {
        return refuse("it holds something that is not a lowercase hexadecimal digit");
    }
    // The relationship LINKINFO.md says a consumer checks. Two fields that are
    // supposed to agree and do not is a manifest somebody edited by hand.
    if !digest.starts_with(value) {
        return Err(LinkInfoError::FingerprintDoesNotMatchDigest {
            manifest: at.to_string(),
            fingerprint: value.to_string(),
            digest: digest.to_string(),
        });
    }
    u64::from_str_radix(value, 16).map_err(|_| LinkInfoError::Fingerprint {
        manifest: at.to_string(),
        value: value.to_string(),
        why: "it is not a base-16 number",
    })
}

/// A hexadecimal digit as this contract spells one: lowercase, and nothing
/// else. Uppercase is refused rather than normalised, because a manifest that
/// spells it differently is a manifest somebody produced by hand.
fn is_lowercase_hex(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, 'a'..='f')
}

fn check_digest(value: &str, at: &str) -> Result<(), LinkInfoError> {
    let looks_right = value.len() == 64 && value.chars().all(is_lowercase_hex);
    if looks_right {
        Ok(())
    } else {
        Err(LinkInfoError::HeaderDigest {
            manifest: at.to_string(),
            value: value.to_string(),
        })
    }
}

fn required<T>(value: Option<T>, field: &'static str, at: &str) -> Result<T, LinkInfoError> {
    value.ok_or_else(|| LinkInfoError::MissingField {
        manifest: at.to_string(),
        field,
    })
}

fn required_text(
    value: Option<String>,
    field: &'static str,
    at: &str,
) -> Result<String, LinkInfoError> {
    let text = required(value, field, at)?;
    if text.is_empty() {
        return Err(LinkInfoError::EmptyField {
            manifest: at.to_string(),
            field,
        });
    }
    Ok(text)
}

/// One of the seven plain string fields: present, not empty, and with nothing
/// in it that would split a line.
fn printable_text(
    value: Option<String>,
    field: &'static str,
    at: &str,
) -> Result<String, LinkInfoError> {
    let text = required_text(value, field, at)?;
    check_printable(&text, field, at)?;
    Ok(text)
}

/// Refuses a control character anywhere in a value this reader repeats.
///
/// Same rule as the library names and the archive root, applied to the fields
/// that were left out of both. Deliberately narrow: everything a version, a
/// triple or an SDK number could legitimately hold is printable, so this
/// refuses `char::is_control` and nothing else. A space, a `+`, a `~` and every
/// non-ASCII letter all survive a single directive line and are none of this
/// check's business.
pub fn check_printable(value: &str, field: &'static str, at: &str) -> Result<(), LinkInfoError> {
    match value.chars().find(|c| c.is_control()) {
        None => Ok(()),
        Some(bad) => Err(LinkInfoError::Unprintable {
            manifest: at.to_string(),
            field,
            value: value.to_string(),
            bad,
        }),
    }
}

fn required_u32(value: Option<u64>, field: &'static str, at: &str) -> Result<u32, LinkInfoError> {
    let number = required(value, field, at)?;
    u32::try_from(number).map_err(|_| LinkInfoError::NumberOutOfRange {
        manifest: at.to_string(),
        field,
        value: number,
    })
}

/// One of the four static fields, which has to be there and has to be a path.
fn static_text(
    value: Option<String>,
    field: &'static str,
    at: &str,
) -> Result<String, LinkInfoError> {
    let text = value.ok_or_else(|| LinkInfoError::StaticFieldMissing {
        manifest: at.to_string(),
        field,
    })?;
    if text.is_empty() {
        return Err(LinkInfoError::EmptyField {
            manifest: at.to_string(),
            field,
        });
    }
    Ok(text)
}

/// One of the four static fields that is a list, which may legitimately be
/// empty: an empty array is a measurement of none.
fn static_present<T>(value: Option<T>, field: &'static str, at: &str) -> Result<T, LinkInfoError> {
    value.ok_or_else(|| LinkInfoError::StaticFieldMissing {
        manifest: at.to_string(),
        field,
    })
}

fn refuse_if_present(present: bool, field: &'static str, at: &str) -> Result<(), LinkInfoError> {
    if present {
        Err(LinkInfoError::StaticFieldUnexpected {
            manifest: at.to_string(),
            field,
        })
    } else {
        Ok(())
    }
}
