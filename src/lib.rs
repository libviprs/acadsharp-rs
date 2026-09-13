//! Safe Rust decoder for DWG, backed by the ACadSharp NativeAOT artifacts
//! published by [`libviprs-dep`].
//!
//! The safe API is still being built. What exists today is [`batch`], the
//! decoder for the VACB wire protocol the native library writes.
//!
//! The boundary this crate owns: it exposes a safe, idiomatic Rust API and
//! never lets .NET or ACadSharp internals reach its callers.
//!
//! [`libviprs-dep`]: https://github.com/libviprs/libviprs-dep
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod batch;
