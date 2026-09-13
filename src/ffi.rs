//! The raw C boundary, one declaration per thing in `viprs_acadsharp.h`.
//!
//! This module is a transcription and nothing else. It holds no logic, no
//! allocation and no interpretation: the safe API that wraps it lives
//! elsewhere, and everything in here has the header's spelling, the header's
//! argument order and the header's widths.
//!
//! The rules it inherits, each one because its absence has broken a C ABI
//! somewhere before:
//!
//! - No `bool`. C `bool` and C# `bool` do not agree on width and neither
//!   language fixes it. Flags are `u8`, 0 or 1, and anything other than 0 is
//!   to be read as 1 rather than as an error.
//! - No `usize`. Lengths, counts and capacities are `u64` on every target. A
//!   width that changes between two builds of one consumer is a width nobody
//!   tests.
//! - No Rust `enum` over a native number. Result codes are `u32` constants, so
//!   a value the native side adds later is still representable instead of
//!   being instant undefined behaviour in a transmute.
//! - No references and no slices in a signature. Raw pointers only, so the
//!   aliasing and validity rules Rust attaches to a reference are never
//!   claimed about memory the callee owns.
//!
//! Every struct here opens with `struct_size` and `struct_version`, both set by
//! the caller. A callee handed a `struct_size` it does not recognise returns
//! [`VIPRS_ACAD_INVALID_ARGUMENT`] rather than reading past what was
//! allocated, and that is what lets the boundary add a field later without
//! breaking a consumer compiled against today's header.
//!
//! [`tests/ffi_layout.rs`] parses the vendored header and checks every offset
//! and size in here against it, so a field that drifts is a failing test rather
//! than a plausible-looking number read out of its neighbour's bytes.
//!
//! Nothing in here decides anything. The one piece of policy this crate has so
//! far, the comparison between what the library reports and what the vendored
//! header declares, lives in [`crate::abi`], because a transcription that
//! reads a constant out of its own parent module has stopped being one.
//!
//! [`tests/ffi_layout.rs`]: https://github.com/libviprs/acadsharp-rs/blob/main/tests/ffi_layout.rs

// The header's names, kept exactly. A Rust-cased alias for each would be a
// second spelling of the same thing, and a boundary where you have to remember
// which spelling you are looking at is a boundary somebody gets wrong.
#![allow(non_camel_case_types)]

// ---------------------------------------------------------------------------
// Result codes
//
// Plain `u32` constants, frozen by number in ABI.md. Downstream switches on the
// number and never parses a string for control flow, because there is no error
// string anywhere on this boundary in either direction.
// ---------------------------------------------------------------------------

/// The call did what it says.
pub const VIPRS_ACAD_OK: u32 = 0;
/// A null pointer, a zero length where one is not allowed, an index out of
/// range, a handle the library did not issue, or a `struct_size` it does not
/// recognise. Nothing was written and nothing was allocated.
pub const VIPRS_ACAD_INVALID_ARGUMENT: u32 = 1;
/// The input is a format, or a version of one, this build does not read.
pub const VIPRS_ACAD_UNSUPPORTED_FORMAT: u32 = 2;
/// The input is the right format and is damaged.
pub const VIPRS_ACAD_CORRUPT_INPUT: u32 = 3;
/// Reserved for a caller that asks for one specific thing this build cannot
/// produce. A drawing full of shapes the decoder has no record type for does
/// not fail: it emits a warning record and carries on.
pub const VIPRS_ACAD_UNSUPPORTED_ENTITY: u32 = 4;
/// An allocation inside the library failed.
pub const VIPRS_ACAD_OUT_OF_MEMORY: u32 = 5;
/// The cancel flag was non-zero. Terminal for a decode.
pub const VIPRS_ACAD_CANCELED: u32 = 6;
/// A bug in the library. Everything the implementation can throw, in whatever
/// language it happens to be written in, arrives here. Terminal for a decode.
pub const VIPRS_ACAD_INTERNAL_ERROR: u32 = 7;
/// The fingerprint handshake failed, or a stream carries a wire version this
/// consumer does not parse.
pub const VIPRS_ACAD_ABI_MISMATCH: u32 = 8;
/// A bound in [`viprs_acad_limits_v1`] was reached, and nothing else. Terminal
/// for a decode.
pub const VIPRS_ACAD_LIMIT_EXCEEDED: u32 = 9;
/// The caller's memory cannot hold what this call would write. The size needed
/// comes back through `required` or through `written`, nothing at all was
/// written, and the call may be retried with more room. Deliberately not
/// terminal: it is about the caller's buffer and not about the decode.
pub const VIPRS_ACAD_BUFFER_TOO_SMALL: u32 = 10;

/// Every result code above, name and value.
///
/// `tests/ffi_codes.rs` compares this against the header's own list in both
/// directions, so a code the header gained and this module has not is a
/// failing test rather than a number the crate meets and does not recognise.
/// The entries hold the constants themselves rather than copies, so only a
/// name can be wrong here, and the same test catches that too.
pub const RESULT_CODES: &[(&str, u32)] = &[
    ("VIPRS_ACAD_OK", VIPRS_ACAD_OK),
    ("VIPRS_ACAD_INVALID_ARGUMENT", VIPRS_ACAD_INVALID_ARGUMENT),
    (
        "VIPRS_ACAD_UNSUPPORTED_FORMAT",
        VIPRS_ACAD_UNSUPPORTED_FORMAT,
    ),
    ("VIPRS_ACAD_CORRUPT_INPUT", VIPRS_ACAD_CORRUPT_INPUT),
    (
        "VIPRS_ACAD_UNSUPPORTED_ENTITY",
        VIPRS_ACAD_UNSUPPORTED_ENTITY,
    ),
    ("VIPRS_ACAD_OUT_OF_MEMORY", VIPRS_ACAD_OUT_OF_MEMORY),
    ("VIPRS_ACAD_CANCELED", VIPRS_ACAD_CANCELED),
    ("VIPRS_ACAD_INTERNAL_ERROR", VIPRS_ACAD_INTERNAL_ERROR),
    ("VIPRS_ACAD_ABI_MISMATCH", VIPRS_ACAD_ABI_MISMATCH),
    ("VIPRS_ACAD_LIMIT_EXCEEDED", VIPRS_ACAD_LIMIT_EXCEEDED),
    ("VIPRS_ACAD_BUFFER_TOO_SMALL", VIPRS_ACAD_BUFFER_TOO_SMALL),
];

// ---------------------------------------------------------------------------
// Opaque handles
//
// Created and destroyed only by the calls below. The caller never dereferences
// one, never does arithmetic on one, and never hands one to free(). A future
// version of the library may make them plain indices.
// ---------------------------------------------------------------------------

/// An open document. Opaque: it has no fields and no size, so nothing can be
/// read out of it by accident.
#[repr(C)]
pub struct viprs_acad_handle {
    _private: [u8; 0],
}

/// A decode in progress over one view. Opaque, same as above.
#[repr(C)]
pub struct viprs_acad_decode_handle {
    _private: [u8; 0],
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Every bound the decoder enforces, set by the caller.
///
/// Drawing files are untrusted input and the library is not a sandbox, so the
/// host makes its own tradeoff rather than inheriting one. A null pointer to
/// either open call means the documented defaults, and a zero field means the
/// default for that field, so a caller can set one bound without knowing the
/// rest. Exceeding any of them is [`VIPRS_ACAD_LIMIT_EXCEEDED`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct viprs_acad_limits_v1 {
    /// `size_of` this struct, set by the caller.
    pub struct_size: u32,
    /// 1, set by the caller.
    pub struct_version: u32,
    /// Refused by the open calls before the input is read.
    pub max_input_bytes: u64,
    /// Counted across the whole decode, block expansion included.
    pub max_entities: u64,
    /// Longest single UTF-8 string a text or warning record may carry.
    pub max_string_bytes: u64,
    /// Vertices in one polyline record.
    pub max_polyline_points: u64,
    /// Nesting depth of an expansion. The only reason this field exists is
    /// that the alternative to a bounded refusal is a stack overflow.
    pub max_block_depth: u32,
    /// Declared padding. The header spells padding out rather than leaving it
    /// implicit.
    pub reserved0: u32,
    /// Total bytes the decode may emit across every batch.
    pub max_output_bytes: u64,
}

/// What a build can actually do, asked at run time.
///
/// Asked rather than inferred from the version this crate compiled against.
/// The drawing-format range in particular comes from the backing reader and
/// moves when it does.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct viprs_acad_capabilities_v1 {
    /// `size_of` this struct, set by the caller.
    pub struct_size: u32,
    /// 1, set by the caller.
    pub struct_version: u32,
    /// The `VIPRS_ACAD_ABI_VERSION` of this build.
    pub abi_version: u32,
    /// The `VIPRS_ACAD_WIRE_VERSION` of this build.
    pub wire_version: u32,
    /// Inclusive AC10xx code, the oldest drawing version this build reads.
    pub dwg_version_min: u32,
    /// Inclusive AC10xx code, the newest drawing version this build reads.
    pub dwg_version_max: u32,
    /// 1 when the build can flatten a nested insertion into transformed
    /// primitives. Read anything other than 0 as 1.
    pub supports_block_expansion: u8,
    /// 1 when reader notifications reach the stream as warning records. Read
    /// anything other than 0 as 1.
    pub supports_warnings: u8,
    /// Declared padding.
    pub reserved0: u8,
    /// Declared padding.
    pub reserved1: u8,
    /// Declared padding.
    pub reserved2: u32,
}

/// One view: model space, or one of the paper-space layouts.
///
/// The name is written into a caller buffer rather than returned as a pointer,
/// so nothing the callee owns outlives the call.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct viprs_acad_view_info_v1 {
    /// `size_of` this struct, set by the caller.
    pub struct_size: u32,
    /// 1, set by the caller.
    pub struct_version: u32,
    /// Echoed back, so a filled struct can be logged on its own.
    pub index: u32,
    /// 0 model, 1 layout, 2 unknown. A `u32` rather than an enum, so a kind
    /// the library adds later still arrives as a number.
    pub kind: u32,
    /// Drawing-unit extents. All zero when the view is empty.
    pub min_x: f64,
    /// Drawing-unit extents. All zero when the view is empty.
    pub min_y: f64,
    /// Drawing-unit extents. All zero when the view is empty.
    pub max_x: f64,
    /// Drawing-unit extents. All zero when the view is empty.
    pub max_y: f64,
    /// Entities before block expansion, for progress reporting only. It is an
    /// upper bound on nothing and a lower bound on nothing, so it must never
    /// size a buffer or decide that a decode finished.
    pub entity_count: u64,
}

impl viprs_acad_limits_v1 {
    /// The `struct_version` this build of the crate fills in.
    ///
    /// The header spells this one in prose rather than in a `#define`, so it
    /// is the single number in the crate I could not derive from the header's
    /// bytes. It moves when the boundary grows a second version of this
    /// struct, and the callee is the one that decides whether it recognises
    /// the pair.
    pub const STRUCT_VERSION: u32 = 1;
}

impl Default for viprs_acad_limits_v1 {
    /// Every bound left to the library, in a struct it accepts.
    ///
    /// `struct_size` and `struct_version` are filled in, and every bound is
    /// zero, which ABI.md defines as "you pick". A derived `Default` gets the
    /// first two wrong: `struct_size` of 0 is a size the callee does not
    /// recognise, and it answers [`VIPRS_ACAD_INVALID_ARGUMENT`] rather than
    /// reading past what was allocated. Measured against the real library,
    /// which is also what `tests/ffi_handshake.rs` asserts, so this cannot
    /// quietly go back to being zeros.
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            struct_version: Self::STRUCT_VERSION,
            max_input_bytes: 0,
            max_entities: 0,
            max_string_bytes: 0,
            max_polyline_points: 0,
            max_block_depth: 0,
            reserved0: 0,
            max_output_bytes: 0,
        }
    }
}

impl viprs_acad_capabilities_v1 {
    /// The `struct_version` this build of the crate fills in. See
    /// [`viprs_acad_limits_v1::STRUCT_VERSION`].
    pub const STRUCT_VERSION: u32 = 1;
}

impl Default for viprs_acad_capabilities_v1 {
    /// An out-struct the library will fill, rather than one it refuses.
    ///
    /// Everything but the two header fields is zero, because every one of them
    /// is an answer the callee writes.
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            struct_version: Self::STRUCT_VERSION,
            abi_version: 0,
            wire_version: 0,
            dwg_version_min: 0,
            dwg_version_max: 0,
            supports_block_expansion: 0,
            supports_warnings: 0,
            reserved0: 0,
            reserved1: 0,
            reserved2: 0,
        }
    }
}

impl viprs_acad_view_info_v1 {
    /// The `struct_version` this build of the crate fills in. See
    /// [`viprs_acad_limits_v1::STRUCT_VERSION`].
    pub const STRUCT_VERSION: u32 = 1;
}

impl Default for viprs_acad_view_info_v1 {
    /// An out-struct the library will fill, rather than one it refuses. Same
    /// as the capabilities one: two fields set, the answers left at zero.
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            struct_version: Self::STRUCT_VERSION,
            index: 0,
            kind: 0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            entity_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
//
// Edition 2024 needs `unsafe extern`, and every item in the block is unsafe to
// call unless it says otherwise. The block is not gated on
// `cfg(acadsharp_linked)`: a declaration nothing calls produces no relocation,
// so the crate links fine without the archive, and keeping the declarations
// visible means `Docs` renders the whole boundary in a job that never links.
// ---------------------------------------------------------------------------

unsafe extern "C" {
    /// The `VIPRS_ACAD_ABI_VERSION` this library was built as. Never fails.
    #[must_use = "this call does nothing but hand back the number, so dropping it is dropping the whole call"]
    pub fn viprs_acad_abi_version() -> u32;

    /// The first eight bytes of the sha256 of the header this library was
    /// built against, big-endian. Never fails.
    #[must_use = "this call does nothing but hand back the number, so dropping it is dropping the whole call"]
    pub fn viprs_acad_abi_fingerprint() -> u64;

    /// Fills `out` and writes the pinned ACadSharp version into
    /// `acadsharp_version_utf8`.
    ///
    /// Call with a null buffer and a `cap` of 0 to learn the required length
    /// through `required`; that sizing call returns [`VIPRS_ACAD_OK`] and
    /// fills the struct as well. A non-null buffer shorter than the string
    /// writes nothing at all and returns [`VIPRS_ACAD_BUFFER_TOO_SMALL`].
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_get_capabilities_v1(
        out: *mut viprs_acad_capabilities_v1,
        acadsharp_version_utf8: *mut u8,
        cap: u64,
        required: *mut u64,
    ) -> u32;

    /// Opens a document from a filesystem path.
    ///
    /// `path` is UTF-8 and is not null terminated; `path_len` is its byte
    /// length. `limits` may be null for the defaults.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_open_path_utf8(
        path: *const u8,
        path_len: u64,
        limits: *const viprs_acad_limits_v1,
        out: *mut *mut viprs_acad_handle,
    ) -> u32;

    /// Opens a document from caller-owned bytes. The caller owns `data` for the
    /// duration of this call only.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_open_memory(
        data: *const u8,
        data_len: u64,
        limits: *const viprs_acad_limits_v1,
        out: *mut *mut viprs_acad_handle,
    ) -> u32;

    /// How many views the document holds. Indices run from zero to one less
    /// than this.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_view_count(h: *mut viprs_acad_handle, out_count: *mut u32) -> u32;

    /// Fills `out` for one view and writes its name into `name_utf8`, using the
    /// same buffer convention as the capabilities call.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_get_view_info_v1(
        h: *mut viprs_acad_handle,
        index: u32,
        out: *mut viprs_acad_view_info_v1,
        name_utf8: *mut u8,
        name_cap: u64,
        name_required: *mut u64,
    ) -> u32;

    /// Begins decoding one view into the batch stream.
    ///
    /// `cancel_flag` is caller-owned and may be null. When non-null the decoder
    /// reads it between batches and never writes it, any non-zero value stops
    /// the decode with [`VIPRS_ACAD_CANCELED`], and it must outlive the decode
    /// handle.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_decode_begin(
        h: *mut viprs_acad_handle,
        view_index: u32,
        cancel_flag: *const u32,
        out: *mut *mut viprs_acad_decode_handle,
    ) -> u32;

    /// Writes the next batch into `buf`.
    ///
    /// `written` is the byte count produced and `done` is 1 when the stream is
    /// complete. Zero both before the call and read them only on
    /// [`VIPRS_ACAD_OK`] and [`VIPRS_ACAD_BUFFER_TOO_SMALL`]: every other code
    /// leaves them undefined as far as this boundary is concerned. That is a
    /// rule for the caller rather than a promise from the callee, and the
    /// difference matters, because the pinned library does in fact write both
    /// (as zero) on [`VIPRS_ACAD_INVALID_ARGUMENT`], [`VIPRS_ACAD_CANCELED`]
    /// and [`VIPRS_ACAD_LIMIT_EXCEEDED`]. Measured, on this archive, today. A
    /// transcription that turns that into a guarantee is inventing one the
    /// contract does not make and the next build need not keep.
    ///
    /// A batch never spans two calls: a `cap`
    /// too small for the next batch returns [`VIPRS_ACAD_BUFFER_TOO_SMALL`]
    /// with the needed size in `written`, having written and consumed nothing.
    /// A `cap` of 0 is [`VIPRS_ACAD_INVALID_ARGUMENT`] instead, because a zero
    /// length is a caller mistake everywhere on this boundary.
    ///
    /// Every other refusal is latched: once a decode fails for a reason of its
    /// own, every later call on the same handle returns that same code, writes
    /// nothing and reports `done` 0.
    #[must_use = "a dropped result code is a call nobody checked, and the next thing that happens is a null handle getting dereferenced"]
    pub fn viprs_acad_decode_next_batch(
        d: *mut viprs_acad_decode_handle,
        buf: *mut u8,
        cap: u64,
        written: *mut u64,
        done: *mut u8,
    ) -> u32;

    /// Releases a decode handle. Null is a no-op. Never fails.
    pub fn viprs_acad_decode_close(d: *mut viprs_acad_decode_handle);

    /// Releases a document handle and every decode handle still open on it.
    /// Null is a no-op. Never fails.
    pub fn viprs_acad_close(h: *mut viprs_acad_handle);
}
