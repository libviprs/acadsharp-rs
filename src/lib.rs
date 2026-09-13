//! Safe Rust decoder for DWG, backed by the ACadSharp NativeAOT artifacts
//! published by [`libviprs-dep`].
//!
//! Callers get Rust types. Neither .NET nor ACadSharp internals reach them,
//! and neither does a raw pointer, an `ffi` type or an `unsafe` block: the one
//! place `unsafe` lives outside the transcription of the C header is a private
//! module that owns the two handles and nothing else.
//!
//! ```rust
//! use acadsharp_rs::{Decoder, Document, Item, Limits, Primitive};
//!
//! fn main() -> Result<(), acadsharp_rs::Error> {
//!     // The handshake, once. A library built from a different header than the
//!     // one this crate vendored is refused here, rather than four bytes into
//!     // a struct that looks plausible.
//!     let decoder = match Decoder::new() {
//!         Ok(decoder) => decoder,
//!         // Nothing was linked. A consumer cannot write that cfg themselves,
//!         // so it arrives as a value rather than as a missing type.
//!         Err(error) if error.is_unlinked() => return Ok(()),
//!         Err(error) => return Err(error),
//!     };
//!     println!("ACadSharp {}", decoder.capabilities().acadsharp_version());
//!
//!     // `VIPRSSYN` is the library's own synthetic document, so this example
//!     // needs no drawing file. For a real one use
//!     // `Document::open_path(&decoder, "plan.dwg", &limits)`, which hands the
//!     // path across as bytes and never reads the file in Rust.
//!     let limits = Limits::new().with_max_polyline_points(100_000);
//!     let document = Document::open_bytes(&decoder, b"VIPRSSYN", &limits)?;
//!
//!     for view in document.views()? {
//!         println!("view {} is {:?}, called {}", view.index(), view.kind(), view.name());
//!     }
//!
//!     let mut lines = 0usize;
//!     let mut stream = document.decode(0)?;
//!     for item in &mut stream {
//!         match item? {
//!             Item::Primitive(Primitive::Line(line)) => {
//!                 lines += 1;
//!                 let _ = (line.start, line.end);
//!             }
//!             Item::Warning(warning) => println!("{}: {}", warning.code, warning.message),
//!             _ => {}
//!         }
//!     }
//!
//!     // The totals are the only proof the decode was not truncated, so ask
//!     // rather than trusting that the loop ended for a good reason.
//!     assert!(stream.is_complete());
//!     println!("{lines} lines out of {} records", stream.records_seen());
//!     Ok(())
//! }
//! ```
//!
//! # The shape of it
//!
//! [`Decoder`] runs the ABI handshake once and reads what the build can do.
//! [`Document`] owns an open drawing and hands out [`View`]s.
//! [`PrimitiveStream`] walks one view, pulling one native batch at a time into
//! a buffer the caller never sees and yielding owned [`Item`]s out of it.
//! Nothing accumulates across batches.
//!
//! Three things are worth knowing before the first call.
//!
//! **The totals are the completeness proof.** A decode that stopped early
//! yields its error and then [`None`], and [`None`] on its own looks exactly
//! like an ending. [`PrimitiveStream::is_complete`] compares what came out
//! with what the stream's own `DocumentEnd` says should have.
//!
//! **Warnings are data, in the stream.** A decode that emits a hundred of them
//! and finishes succeeded. They arrive inline because reading one often means
//! reading what sits beside it: [`WarningCode::EMPTY_VIEW`] alone is a view
//! with nothing in it, and the same code with other warnings around it is a
//! view something went wrong reading.
//!
//! **Nothing is tessellated or projected.** Arcs, circles, ellipses and
//! splines keep their parameters, a polyline's bulges cross as bulges, and
//! every coordinate is 3D. Turning a curve into segments needs a tolerance and
//! flattening 3D into a plane needs an axis, and neither is a choice a producer
//! can make for a consumer it cannot see. [`item`] has the recipes.
//!
//! # The layers underneath
//!
//! [`batch`] is the zero-copy decoder for the VACB wire protocol, borrowed
//! views and no allocation, for a caller who already holds bytes. [`abi`]
//! holds the three constants that pin this crate to one version of the
//! vendored header, and [`abi::check`], the comparison that refuses a library
//! built from another one. The raw transcription of `viprs_acadsharp.h` and
//! every call into the library sit below both of those, and neither is
//! something a caller reaches for.
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
//! header that moved.
//!
//! # Linking the native library
//!
//! Point `ACADSHARP_NATIVE_DIR` at an unpacked `libviprs-dep` archive, the
//! directory holding `lib/` and `metadata/LINKINFO.json`, and the build script
//! emits the link lines and sets `cfg(acadsharp_linked)`. With no archive the
//! crate still builds, checks, tests and documents: the public surface is the
//! same either way, and [`Decoder::new`] answers [`Error::Unlinked`] instead
//! of disappearing. A surface that changed shape with an environment variable
//! would be one a consumer could not write a `cfg` for.
//!
//! [`libviprs-dep`]: https://github.com/libviprs/libviprs-dep
#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
// The layering rule, as a lint rather than as a grep. Two modules are allowed
// an `unsafe` block and they say so on the line that declares them, so widening
// that is a diff a reviewer sees rather than a file a hand-maintained list in a
// test happened not to name. `tests/api_surface.rs` keeps the text scans for
// the `*mut` and `ffi::` checks, which no lint covers.
#![deny(unsafe_code)]

pub mod abi;
pub mod batch;
pub mod diagnostics;
pub mod item;

mod cancel;
mod capabilities;
mod document;
mod error;
mod limits;
mod stream;

// The raw transcription of `viprs_acadsharp.h`, and the one private module
// that calls it. These two lines are the whole of the exemption from
// `deny(unsafe_code)` above.
//
// `ffi` is `pub` because this crate's own integration tests compile against it
// from outside the crate, and `#[doc(hidden)]` because nothing else should.
// Documenting it would contradict the sentence at the top of this file, since
// `*mut acadsharp_rs::ffi::viprs_acad_handle` compiles from a consumer crate,
// and it would freeze eleven `unsafe extern "C"` signatures into a 0.1.0
// semver promise, which turns the next header revision into a breaking API
// change. `batch` stays documented because it is safe, standalone and useful
// on its own, and `abi` because the three constants and `check` are things a
// consumer legitimately reads.
#[doc(hidden)]
#[allow(unsafe_code)]
pub mod ffi;

#[allow(unsafe_code)]
mod sys;

// `#[doc(inline)]` on the three lines that re-export out of a public module.
// Without it rustdoc renders nineteen of the most important types in this
// crate as a bare link with no description, on the front page, next to the
// ones re-exported out of a private module, which it inlines automatically and
// which therefore get a sentence each. That difference is an artefact of where
// a type happens to live and says nothing a reader wants to know.
#[doc(inline)]
pub use abi::{EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};
#[doc(inline)]
pub use batch::{DocumentBegin, DocumentEnd, ViewEnd};
#[doc(inline)]
pub use item::{
    Arc, Circle, Ellipse, Item, ItemHandle, Line, Origin, Polyline, Primitive, Spline, Text,
    Warning, WarningCode,
};

/// The wire's own record numbers, which are the contract a consumer switches
/// on. Re-exported here because [`Item::record_type`] hands one back and a
/// caller should not have to reach into [`batch`] to name it.
pub use batch::record_type;
pub use cancel::CancelToken;
pub use capabilities::Capabilities;
pub use document::{Decoder, Document, Extents, View, ViewKind};
pub use error::{Error, Result};
pub use limits::Limits;
pub use stream::PrimitiveStream;
