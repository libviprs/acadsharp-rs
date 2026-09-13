//! Safe Rust decoder for DWG, backed by the ACadSharp NativeAOT artifacts
//! published by [`libviprs-dep`].
//!
//! Nothing is implemented yet. This crate exists so the repository's CI
//! conventions have something real to run against, and so the ABI work in
//! `libviprs-dep` has a consumer to be conformance-tested from.
//!
//! The boundary this crate owns: it exposes a safe, idiomatic Rust API and
//! never lets .NET or ACadSharp internals reach its callers.
//!
//! [`libviprs-dep`]: https://github.com/libviprs/libviprs-dep
#![forbid(unsafe_op_in_unsafe_fn)]

/// The VIPRS CAD ABI version this crate is written against.
///
/// The native library exposes the same number through `viprs_acad_abi_version`,
/// and a mismatch is a hard error rather than something to negotiate at
/// runtime. Bumping the native ABI without bumping this is the failure the
/// conformance consumer exists to catch.
pub const EXPECTED_ABI_VERSION: u32 = 1;
