//! `metadata/LINKINFO.json`, turned from text into fields by a real JSON
//! parser.
//!
//! This module is deliberately thin. All it does is read the document and hand
//! the fields to [`crate::manifest::Raw::validate`], which owns every rule
//! about what those fields have to say. The split is what lets the rules be
//! tested as plain functions: `serde` and `serde_json` are `[build-dependencies]`
//! and never reach a consumer's binary, which also means a test binary cannot
//! link them, so `tests/build_manifest.rs` tests the rules directly and
//! `tests/build_script.rs` drives this layer by running the real build script
//! over the real manifests.
//!
//! # Why a parser rather than the reader this replaces
//!
//! The first version of this file read one field by hand: find the key, find
//! the brackets, split on commas, strip the quotes. It accepted a raw newline
//! inside a string, which is invalid JSON that any real parser refuses, and
//! that was an injection. A `shared_system_libraries` entry of
//! `"m\ncargo::rustc-link-arg=--totally-bogus-linker-flag"` produced those exact
//! directives in the build script's output and the flag reached the real link
//! line. `rustc-link-arg` is arbitrary linker flags, so that was build-time code
//! execution driven by a file arriving inside a downloaded tarball.
//!
//! Hand-rolling a reader for a twenty-key manifest is two hundred lines that
//! then need fuzzing for no runtime benefit. So: a parser, under
//! `[build-dependencies]`, and the validation beside it in a file with no
//! dependencies at all.
//!
//! # Unknown keys are skipped, not refused
//!
//! `schema_version` moves when a field is removed, renamed or has its meaning
//! changed, and adding one does not move it. That is the other half of the
//! version rule: an archive may carry a key this consumer has never heard of,
//! and this consumer skips it and keeps working. So no `deny_unknown_fields`
//! here. A version that refused every unknown key would refuse every newer
//! archive for a field none of them read.

// This file and its two neighbours are compiled into two different crates: the
// build script, and the test binaries that check them. Each uses a different
// subset, so something unused here is used over there.
#![allow(dead_code)]

use std::path::Path;

use serde::Deserialize;

use crate::manifest::{LinkInfo, LinkInfoError, Raw};

/// The manifest as JSON says it, one `Option` per key.
///
/// Every field is optional at this layer on purpose. "Absent" is a fact the
/// rules next door need to be able to see, and a `#[serde(default)]` here plus
/// a required-field check there gives a refusal that names the field, where
/// serde's own missing-field error would name it inside a parser message.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Wire {
    schema_version: Option<u64>,
    artifact_version: Option<String>,
    acadsharp_version: Option<String>,
    acadsharp_commit: Option<String>,
    dotnet_sdk: Option<String>,
    target: Option<String>,
    platform: Option<String>,
    cpu: Option<String>,
    abi_version: Option<u64>,
    wire_version: Option<u64>,
    abi_header_sha256: Option<String>,
    abi_fingerprint: Option<String>,
    shared_library: Option<String>,
    shared_system_libraries: Option<Vec<String>>,
    static_library: Option<String>,
    static_init_library: Option<String>,
    static_certified: Option<bool>,
    static_system_libraries: Option<Vec<String>>,
    static_link_args: Option<Vec<String>>,
    dwg_version_min: Option<u64>,
    dwg_version_max: Option<u64>,
}

impl From<Wire> for Raw {
    fn from(wire: Wire) -> Self {
        // Field by field rather than a blanket conversion, so adding a key to
        // one struct and not the other does not compile.
        Self {
            schema_version: wire.schema_version,
            artifact_version: wire.artifact_version,
            acadsharp_version: wire.acadsharp_version,
            acadsharp_commit: wire.acadsharp_commit,
            dotnet_sdk: wire.dotnet_sdk,
            target: wire.target,
            platform: wire.platform,
            cpu: wire.cpu,
            abi_version: wire.abi_version,
            wire_version: wire.wire_version,
            abi_header_sha256: wire.abi_header_sha256,
            abi_fingerprint: wire.abi_fingerprint,
            shared_library: wire.shared_library,
            shared_system_libraries: wire.shared_system_libraries,
            static_library: wire.static_library,
            static_init_library: wire.static_init_library,
            static_certified: wire.static_certified,
            static_system_libraries: wire.static_system_libraries,
            static_link_args: wire.static_link_args,
            dwg_version_min: wire.dwg_version_min,
            dwg_version_max: wire.dwg_version_max,
        }
    }
}

/// Reads one manifest off disk and decides whether it is one.
pub fn read(manifest: &Path) -> Result<LinkInfo, LinkInfoError> {
    let text = std::fs::read_to_string(manifest).map_err(|e| LinkInfoError::Unreadable {
        manifest: manifest.display().to_string(),
        why: e.to_string(),
    })?;
    parse(&text, manifest)
}

/// The same thing from text, which is the form the tests drive.
pub fn parse(text: &str, manifest: &Path) -> Result<LinkInfo, LinkInfoError> {
    let wire: Wire = serde_json::from_str(text).map_err(|e| LinkInfoError::Json {
        manifest: manifest.display().to_string(),
        why: e.to_string(),
    })?;
    Raw::from(wire).validate(manifest)
}
