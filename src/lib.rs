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
//! The `link-static` feature picks the other way. It is off by default and off
//! is the shared link. It needs an archive whose manifest says
//! `static_certified: true`, which is true only where the producer linked the
//! static archive into a probe program and ran it on that target, and asking
//! for it anywhere else is a refusal naming the target.
//!
//! There is one feature rather than a pair, because cargo features are
//! additive: any crate in a graph may turn one on and none can turn another's
//! off. A `link-shared` beside it would make a graph where one dependency asks
//! for each into a build that stops with advice neither author can act on.
//!
//! ## A binary that depends on this crate needs one more thing
//!
//! **Static is the mode to deploy in and shared is the mode to develop in.**
//!
//! The rpath that finds `libacadsharp_native.so` goes out as
//! `cargo::rustc-link-arg`, and cargo binds that to the emitting package's own
//! binaries, tests and examples. It goes no further. So this crate's tests run,
//! and a binary that depends on this crate links and then dies before `main`
//! with `error while loading shared libraries: libacadsharp_native.so` and exit
//! code 127.
//!
//! Three ways out, best first:
//!
//! 1. A `build.rs` in the crate that produces the binary, emitting its own
//!    rpath. This crate declares `links = "acadsharp_native"`, so cargo hands a
//!    downstream build script `DEP_ACADSHARP_NATIVE_LIB_DIR` (the archive's
//!    `lib/`), `DEP_ACADSHARP_NATIVE_NATIVE_DIR` (its root),
//!    `DEP_ACADSHARP_NATIVE_LINK_KIND` and
//!    `DEP_ACADSHARP_NATIVE_ARTIFACT_VERSION`:
//!
//!    ```text
//!    // build.rs, in the crate that produces the binary
//!    fn main() {
//!        if let Ok(dir) = std::env::var("DEP_ACADSHARP_NATIVE_LIB_DIR") {
//!            println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
//!        }
//!    }
//!    ```
//!
//!    Not a doctest, because it belongs to a different crate than this one.
//!    CI writes exactly this file into a real downstream crate, builds it and
//!    runs the binary, which is a better check than compiling it here would be.
//!
//!    They are set only when an archive resolved, so read them as a `Result`
//!    and carry on without them. This bakes the build machine's path into the
//!    binary, which is right for a developer build and wrong for a shipped one.
//! 2. `link-static`, which puts the library inside the binary and leaves the
//!    loader nothing to do. This is the answer for anything that leaves the
//!    machine that built it.
//! 3. `LD_LIBRARY_PATH` pointing at the archive's `lib/`, set wherever the
//!    binary runs. Quickest to type, easiest to forget on the machine that
//!    matters.
//!
//! [`libviprs-dep`]: https://github.com/libviprs/libviprs-dep
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod batch;
pub mod ffi;

pub use abi::{EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};
