//! Safe Rust decoder for DWG, backed by the ACadSharp NativeAOT artifacts
//! published by [`libviprs-dep`].
//!
//! The safe API is still being built. What exists today is [`ffi`], the raw
//! transcription of `viprs_acadsharp.h`, [`batch`], the decoder for the VACB
//! wire protocol that library writes, and [`abi`], which holds the three
//! constants that pin this crate to one version of that header and the
//! handshake that refuses a library built from a different one.
//!
//! The boundary this crate owns: it exposes a safe, idiomatic Rust API and
//! never lets .NET or ACadSharp internals reach its callers.
//!
//! # The header is vendored, and the constants are derived from it
//!
//! `native/viprs_acadsharp.h` is a byte-for-byte copy of the frozen header,
//! pinned to the `libviprs-dep` commit named in `native/NATIVE_HEADER_REV` and
//! hashed in `native/viprs_acadsharp.h.sha256`. The build script refuses to
//! build if those two disagree.
//!
//! [`EXPECTED_ABI_VERSION`], [`EXPECTED_WIRE_VERSION`] and
//! [`EXPECTED_ABI_FINGERPRINT`] are generated from that file's bytes at build
//! time. Nobody types them, so nothing in the crate can go on agreeing with a
//! header that moved. They live in [`abi`] and are re-exported here, because a
//! leaf module both [`ffi`] and [`batch`] can depend on beats three names at
//! the root that everything reaches up for. [`batch::WIRE_VERSION`] is the same
//! number narrowed once to the `u16` the batch header actually carries.
//!
//! # Linking the native library
//!
//! Point `ACADSHARP_NATIVE_DIR` at an unpacked `libviprs-dep` archive, the
//! directory holding `lib/` and `metadata/LINKINFO.json`, and the build script
//! emits the link lines and sets `cfg(acadsharp_linked)`. Failing that it looks
//! in `$CARGO_HOME/acadsharp-native/<artifact_version>/<platform>-<cpu>/`, and
//! it never downloads anything. With no archive the crate still builds, checks
//! and documents; everything that calls the library is simply compiled out,
//! which is what [`abi::handshake`] being the one gated function in here is
//! about. [`batch`] never links: it reads bytes.
//!
//! The `link-shared` and `link-static` features pick which way. Both are off by
//! default and the default is the shared link. `link-static` needs an archive
//! whose manifest says `static_certified: true`, which is true only where the
//! producer linked the static archive into a probe program and ran it on that
//! target, and asking for it anywhere else is a refusal naming the target.
//!
//! [`libviprs-dep`]: https://github.com/libviprs/libviprs-dep
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod batch;
pub mod ffi;

pub use abi::{EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};
