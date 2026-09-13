//! The VACB batch protocol, wire version 2.
//!
//! [`viprs_acad_decode_next_batch`] writes one batch per call into a buffer the
//! caller owns, and this module is everything needed to read one. It treats the
//! bytes as untrusted even though they come from the paired shim, because a
//! length field is a claim and not a fact, and because the bytes may not have
//! come from that producer at all.
//!
//! [`viprs_acad_decode_next_batch`]: https://github.com/libviprs/libviprs-dep
//!
//! # What it will not do
//!
//! - **It never allocates.** Variable length payloads come back as borrowed
//!   views ([`Vertices`], [`Scalars`], [`str`]) that read one element at a time
//!   out of the batch buffer, so a declared count can never become a
//!   reservation. The only bound this module recognises is bytes in hand.
//! - **It never wraps.** Every length, count, offset and padding computation is
//!   done in `u64`. Every count on this wire is a `u32`, so widening is what
//!   bounds the whole computation: `64 + 24n + 8bc` maxes out around `1.4e11`,
//!   which is why there is no `checked_add` here and no unreachable `None` arm
//!   that reads like safety. In `u32` that same identity wraps at
//!   `n = 2_863_311_531` and lets a 72 byte record claim 68 GB of vertices.
//! - **It never panics.** Every read goes through a bounds checked accessor
//!   that returns [`None`] rather than indexing, and there is no `unwrap`,
//!   `expect` or slice index in the file. `#![forbid(unsafe_code)]` is on the
//!   module.
//! - **It never hangs.** A record shorter than its own header cannot advance
//!   the cursor, so it is refused, and a hard iteration bound of
//!   `payload_length / 8 + 1` turns a mistake in that rule into a failed parse
//!   rather than a job somebody has to kill. That bound and the one other
//!   backstop here report [`BatchError::Internal`], because a bug in this
//!   crate is not the caller's drawing being corrupt.
//!
//! # What it will not refuse
//!
//! Over strictness breaks on the next wire version and is a defect in the same
//! way under strictness is, so this module deliberately reads past: payload
//! `reserved` fields, batch flag bits above bit 0, non-zero string padding,
//! record types it does not know (skipped by `length`, never by a size table),
//! and warning codes it does not know. A [`ViewBegin`]'s extents are exempt
//! from the finiteness rule, and the inverted box a producer writes for a view
//! with no usable extents comes back as [`ViewBegin::bounds`] of [`None`]
//! rather than as a refusal or as a rectangle.
//!
//! # Reading a batch
//!
//! ```
//! use acadsharp_rs::batch::{BatchReader, Record};
//!
//! // One DocumentBegin record: twelve bytes of batch header, then twenty four
//! // of record.
//! let mut bytes = Vec::new();
//! bytes.extend_from_slice(b"VACB");
//! bytes.extend_from_slice(&2u16.to_le_bytes()); // wire_version
//! bytes.extend_from_slice(&1u16.to_le_bytes()); // flags: the last batch
//! bytes.extend_from_slice(&24u32.to_le_bytes()); // payload_length
//! bytes.extend_from_slice(&1u16.to_le_bytes()); // type: DocumentBegin
//! bytes.extend_from_slice(&0u16.to_le_bytes()); // reserved
//! bytes.extend_from_slice(&24u32.to_le_bytes()); // length, header included
//! bytes.extend_from_slice(&3u32.to_le_bytes()); // view_count
//! bytes.extend_from_slice(&1032u32.to_le_bytes()); // drawing_version
//! bytes.extend_from_slice(&0u64.to_le_bytes()); // reserved0
//!
//! let batch = BatchReader::new(&bytes)?;
//! assert!(batch.is_last());
//! assert_eq!(batch.total_len(), bytes.len());
//!
//! let mut records = batch.records();
//! match records.next().transpose()? {
//!     Some(Record::DocumentBegin(d)) => assert_eq!(d.view_count, 3),
//!     other => panic!("expected a DocumentBegin, got {other:?}"),
//! }
//! assert!(records.next().is_none());
//! # Ok::<(), acadsharp_rs::batch::BatchError>(())
//! ```
//!
//! A stream is a run of batches back to back, so walking one is
//! [`BatchReader::new`] followed by advancing the cursor by
//! [`BatchReader::total_len`] until the bytes run out.

#![forbid(unsafe_code)]

use std::fmt;
use std::iter::FusedIterator;

/// The four bytes every batch opens with.
pub const MAGIC: [u8; 4] = *b"VACB";

/// The only wire version this module parses. Anything else is an
/// [`BatchError::AbiMismatch`], because a record type this module does know may
/// have changed shape underneath it.
///
/// This is [`crate::EXPECTED_WIRE_VERSION`], which `build.rs` derives from the
/// bytes of the vendored header, narrowed to the `u16` the batch header
/// actually carries. Two copies of the same number is how a decoder ends up
/// confidently parsing a layout that moved underneath it, so there is one
/// copy, and the narrowing is a compile-time `assert!` rather than a bare
/// `as u16`: a truncating cast at header wire version 65538 would have this
/// module accept a batch declaring 2 while reporting agreement, which is the
/// exact failure the derived constant exists to stop.
pub const WIRE_VERSION: u16 = {
    assert!(
        crate::EXPECTED_WIRE_VERSION <= u16::MAX as u32,
        "the header's wire version does not fit the wire's own u16 field"
    );
    crate::EXPECTED_WIRE_VERSION as u16
};

/// Bytes of batch header before the first record.
pub const BATCH_HEADER_LEN: usize = 12;

/// Bytes of record header before a payload. A record's `length` includes it.
pub const RECORD_HEADER_LEN: u64 = 8;

/// Set in a batch's `flags` on the last batch of a stream. Every other bit is
/// ignored rather than refused.
pub const FLAG_LAST_BATCH: u16 = 1;

/// The wire numbers, which are the contract. A consumer switches on the number
/// and never on a name.
pub mod record_type {
    /// How many views the document has, and the AC10xx code it was read from.
    pub const DOCUMENT_BEGIN: u16 = 1;
    /// One view's index, kind, extents, item count and name.
    pub const VIEW_BEGIN: u16 = 2;
    /// Two endpoints.
    pub const LINE: u16 = 3;
    /// A vertex run, open or closed, with a normal and a bulge per vertex.
    pub const POLYLINE: u16 = 4;
    /// Centre, radius, start and end angle, and a normal.
    pub const ARC: u16 = 5;
    /// Centre, radius, and a normal.
    pub const CIRCLE: u16 = 6;
    /// Centre, major axis, ratio, parameter range, and a normal.
    pub const ELLIPSE: u16 = 7;
    /// Degree, knots, control points and weights.
    pub const SPLINE: u16 = 8;
    /// A closed boundary, carrying [`POLYLINE`]'s payload exactly.
    pub const POLYGON: u16 = 9;
    /// Position, height, rotation, and UTF-8 bytes with a length.
    pub const TEXT: u16 = 10;
    /// A numeric code, a UTF-8 message, and an optional item handle.
    pub const WARNING: u16 = 11;
    /// The view index it closes, and how many records it contained.
    pub const VIEW_END: u16 = 12;
    /// Totals for the whole stream.
    pub const DOCUMENT_END: u16 = 13;
    /// Numbers from here up are the forward probe range. They carry no meaning
    /// in wire version 2 and are skipped by `length`.
    pub const FORWARD_PROBE_MIN: u16 = 32512;
}

// The one legal length of each fixed size record, and the fixed part of each
// counted one. These are `u64` so every identity below is computed in `u64`.
const LEN_DOCUMENT_BEGIN: u64 = 24;
const LEN_LINE: u64 = 72;
const LEN_ARC: u64 = 96;
const LEN_CIRCLE: u64 = 80;
const LEN_ELLIPSE: u64 = 120;
const LEN_VIEW_END: u64 = 24;
const LEN_DOCUMENT_END: u64 = 24;
const VIEW_BEGIN_FIXED: u64 = 64;
const POLYLINE_FIXED: u64 = 64;
const SPLINE_FIXED: u64 = 48;
const TEXT_FIXED: u64 = 72;
const WARNING_FIXED: u64 = 32;

// Why there is no `checked_add` in this file. Every count on the wire is a
// `u32`, so widening to `u64` bounds the whole computation before it starts,
// and these assertions record the bound rather than leaving it to a comment.
const MAX_COUNT: u64 = u32::MAX as u64;
const _: () = assert!(
    POLYLINE_FIXED + 24 * MAX_COUNT + 8 * MAX_COUNT < u64::MAX,
    "the polyline identity must not be able to wrap in u64"
);
const _: () = assert!(
    SPLINE_FIXED + 8 * MAX_COUNT + 24 * MAX_COUNT + 8 * MAX_COUNT < u64::MAX,
    "the spline identity must not be able to wrap in u64"
);
const _: () = assert!(
    TEXT_FIXED + MAX_COUNT + 3 < u64::MAX,
    "a padded string length must not be able to wrap in u64"
);

/// Rounds up to a multiple of four. Padding is computed in `u64` like
/// everything else, and `v` is at most `u32::MAX`, so `v + 3` cannot wrap.
const fn pad4(v: u64) -> u64 {
    (v + 3) & !3
}

/// `64 + 24n + 8bc`, the length a `Polyline` or `Polygon` must declare.
const fn polyline_length(point_count: u64, bulge_count: u64) -> u64 {
    POLYLINE_FIXED + 24 * point_count + 8 * bulge_count
}

/// `48 + 8k + 24c + 8w`, the length a `Spline` must declare.
const fn spline_length(knots: u64, controls: u64, weights: u64) -> u64 {
    SPLINE_FIXED + 8 * knots + 24 * controls + 8 * weights
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a batch was refused.
///
/// This is `#[non_exhaustive]` because a later wire version will have rules
/// this one does not, and adding one should not be a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The first four bytes are not `VACB`.
    BadMagic,
    /// Fewer than twelve bytes, so there is no batch header to read.
    ShortBatchHeader,
    /// `12 + payload_length` runs past the buffer.
    PayloadPastBuffer,
    /// Fewer than eight bytes are left, so there is no record header to read.
    ShortRecordHeader,
    /// A `length` below eight. This is the infinite loop: a record that cannot
    /// advance the cursor past its own header.
    LengthBelowHeader,
    /// A `length` that is not a multiple of four.
    LengthNotMultipleOfFour,
    /// A `length` that claims more than the batch holds.
    LengthPastPayload,
    /// A record header's `reserved` field is not zero.
    RecordReservedNotZero,
    /// A fixed size record whose `length` is not the one length it may have,
    /// or a counted record too short to hold its own count fields.
    WrongFixedLength,
    /// A `Polyline` or `Polygon` whose `length` is not `64 + 24n + 8bc`.
    PolylineLengthMismatch,
    /// A `bulge_count` that is neither zero nor the `point_count`.
    BulgeCount,
    /// A `Spline` whose `length` is not `48 + 8k + 24c + 8w`.
    SplineLengthMismatch,
    /// A `weight_count` that is neither zero nor the `control_count`.
    WeightCount,
    /// A string record whose `length` is not its fixed part plus its declared
    /// byte count padded to a multiple of four.
    StringLengthMismatch,
    /// A declared string that is not UTF-8.
    InvalidUtf8,
    /// A `NaN` or an infinity in a geometry record, types 3 to 10. The producer
    /// promises never to write one, and these bytes may not have come from that
    /// producer.
    NonFiniteFloat,
    /// A `Warning` carrying code zero, which is not a warning code.
    ZeroWarningCode,
}

impl Reason {
    const fn message(self) -> &'static str {
        match self {
            Self::BadMagic => "the first four bytes are not VACB",
            Self::ShortBatchHeader => {
                "there are fewer than twelve bytes, so there is no batch header"
            }
            Self::PayloadPastBuffer => "payload_length runs past the end of the buffer",
            Self::ShortRecordHeader => {
                "fewer than eight bytes are left, so there is no record header"
            }
            Self::LengthBelowHeader => {
                "a record length below eight cannot advance the cursor past its own header"
            }
            Self::LengthNotMultipleOfFour => "a record length must be a multiple of four",
            Self::LengthPastPayload => "a record claims more bytes than the batch holds",
            Self::RecordReservedNotZero => "a record header's reserved field is not zero",
            Self::WrongFixedLength => "this record type does not have this length",
            Self::PolylineLengthMismatch => "a polyline length must be 64 + 24n + 8bc",
            Self::BulgeCount => "a bulge_count is either zero or the point_count",
            Self::SplineLengthMismatch => "a spline length must be 48 + 8k + 24c + 8w",
            Self::WeightCount => "a weight_count is either zero or the control_count",
            Self::StringLengthMismatch => {
                "a string record's length does not match its declared byte count"
            }
            Self::InvalidUtf8 => "a declared string is not UTF-8",
            Self::NonFiniteFloat => "a geometry record carries a NaN or an infinity",
            Self::ZeroWarningCode => "zero is not a warning code",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Why a batch could not be read, and where in it the trouble is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// These bytes are not a batch this wire version describes.
    CorruptInput {
        /// Bytes from the start of the batch. For a batch header rule this is
        /// the field's own offset; for a record rule it is the offset of the
        /// record the refusal is about.
        offset: u64,
        /// Which rule was broken.
        reason: Reason,
    },
    /// The `wire_version` is not one this module parses.
    ///
    /// This is deliberately not [`BatchError::CorruptInput`]. A foreign wire
    /// version is the two ends of this boundary disagreeing and the remedy is
    /// to rebuild one of them, where corrupt input is about the bytes.
    AbiMismatch {
        /// Bytes from the start of the batch, so always 4.
        offset: u64,
        /// The version the batch declares.
        found: u16,
        /// The version this module parses, [`WIRE_VERSION`].
        expected: u16,
    },
    /// A bug in this crate, not a problem with the bytes.
    ///
    /// Two backstops produce this and neither should ever fire. One is the
    /// hard iteration bound, which catches a record that advanced the cursor
    /// by less than it should have; the other is a field running off the end
    /// of a payload whose length this module already checked. Both are
    /// unreachable by construction and both are typed rather than a panic,
    /// because a decoder that panics on a length field is worse than one that
    /// says it got confused.
    ///
    /// They used to be [`BatchError::CorruptInput`], which told a caller their
    /// drawing was corrupt when the drawing was fine and the decoder was not.
    /// Getting one of these means the bytes at `offset` are worth attaching to
    /// a bug report against this crate.
    Internal {
        /// Bytes from the start of the batch to the record being read.
        offset: u64,
    },
}

impl BatchError {
    /// Bytes from the start of the batch to whatever was refused.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        match self {
            Self::CorruptInput { offset, .. }
            | Self::AbiMismatch { offset, .. }
            | Self::Internal { offset } => *offset,
        }
    }

    const fn corrupt(offset: u64, reason: Reason) -> Self {
        Self::CorruptInput { offset, reason }
    }

    const fn internal(offset: u64) -> Self {
        Self::Internal { offset }
    }
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorruptInput { offset, reason } => {
                write!(f, "corrupt batch at offset {offset}: {reason}")
            }
            Self::AbiMismatch {
                offset,
                found,
                expected,
            } => write!(
                f,
                "the batch at offset {offset} declares wire version {found}, and I only parse {expected}"
            ),
            Self::Internal { offset } => write!(
                f,
                "I got confused reading the batch at offset {offset}, which is a bug in acadsharp-rs rather than a problem with these bytes"
            ),
        }
    }
}

impl std::error::Error for BatchError {}

/// Why [`BatchReader::records_from`] would not resume at an offset.
///
/// This is not [`BatchError`] on purpose. Nothing is wrong with the bytes and
/// nothing is wrong with this module; the number it was handed cannot name a
/// record in this batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResumeError {
    /// The offset is inside the twelve byte batch header, so it is in front of
    /// the first record rather than at it.
    BeforeFirstRecord {
        /// The offset that was asked for.
        offset: u64,
        /// The offset of the first record, which is [`BATCH_HEADER_LEN`].
        first_record: u64,
    },
    /// The offset is past the end of this batch.
    PastEndOfBatch {
        /// The offset that was asked for.
        offset: u64,
        /// [`BatchReader::total_len`], which is one past the last byte.
        total_len: u64,
    },
    /// Every record's `length` is a multiple of four, so every record boundary
    /// is one too, and this offset is not.
    NotAligned {
        /// The offset that was asked for.
        offset: u64,
    },
}

impl fmt::Display for ResumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BeforeFirstRecord {
                offset,
                first_record,
            } => write!(
                f,
                "offset {offset} is inside the batch header, and the first record is at {first_record}"
            ),
            Self::PastEndOfBatch { offset, total_len } => write!(
                f,
                "offset {offset} is past this batch, which ends at {total_len}"
            ),
            Self::NotAligned { offset } => write!(
                f,
                "offset {offset} is not a multiple of four past the batch header, so no record starts there"
            ),
        }
    }
}

impl std::error::Error for ResumeError {}

// ---------------------------------------------------------------------------
// Bounds checked reads
// ---------------------------------------------------------------------------

/// A byte range with bounds checked accessors. Nothing in this module indexes a
/// slice, so nothing in it can panic on any input.
#[derive(Clone, Copy)]
struct Fields<'a> {
    bytes: &'a [u8],
}

impl<'a> Fields<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn array<const N: usize>(self, at: usize) -> Option<[u8; N]> {
        let end = at.checked_add(N)?;
        self.bytes.get(at..end)?.try_into().ok()
    }

    fn read_u16(self, at: usize) -> Option<u16> {
        self.array::<2>(at).map(u16::from_le_bytes)
    }

    fn read_u32(self, at: usize) -> Option<u32> {
        self.array::<4>(at).map(u32::from_le_bytes)
    }

    fn read_u64(self, at: usize) -> Option<u64> {
        self.array::<8>(at).map(u64::from_le_bytes)
    }

    fn read_f64(self, at: usize) -> Option<f64> {
        self.array::<8>(at).map(f64::from_le_bytes)
    }

    fn read_f64x3(self, at: usize) -> Option<[f64; 3]> {
        Some([
            self.read_f64(at)?,
            self.read_f64(at.checked_add(8)?)?,
            self.read_f64(at.checked_add(16)?)?,
        ])
    }

    fn read_slice(self, at: usize, len: usize) -> Option<&'a [u8]> {
        let end = at.checked_add(len)?;
        self.bytes.get(at..end)
    }

    fn tail(self, at: usize) -> Option<&'a [u8]> {
        self.bytes.get(at..)
    }
}

/// Every eight byte window of `bytes`, read as an `f64`, is finite. A trailing
/// remainder shorter than eight bytes is not a float and is not examined.
fn all_finite(bytes: &[u8]) -> bool {
    let (chunks, _) = bytes.as_chunks::<8>();
    chunks.iter().all(|c| f64::from_le_bytes(*c).is_finite())
}

// ---------------------------------------------------------------------------
// Borrowed views over the variable length payloads
// ---------------------------------------------------------------------------

/// A run of 3D points, read one at a time out of the batch buffer.
///
/// Nothing is copied and nothing is allocated, so a polyline at the default
/// `max_polyline_points` costs the same as an empty one until something asks
/// for a vertex.
#[derive(Clone, Copy)]
pub struct Vertices<'a> {
    bytes: &'a [u8],
}

impl<'a> Vertices<'a> {
    const STRIDE: usize = 24;

    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// How many points there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len() / Self::STRIDE
    }

    /// Whether there are no points at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The point at `index`, or [`None`] past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<[f64; 3]> {
        let at = index.checked_mul(Self::STRIDE)?;
        Fields::new(self.bytes).read_f64x3(at)
    }

    /// Every point, in wire order.
    #[must_use]
    pub fn iter(&self) -> VertexIter<'a> {
        let (chunks, _) = self.bytes.as_chunks::<24>();
        VertexIter {
            inner: chunks.iter(),
        }
    }

    /// The underlying bytes, for a caller that wants to do its own reads.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Element by element, so two views over different bytes that decode to the
/// same points compare equal. Comparing the bytes would call `0.0` and `-0.0`
/// different, which they are not.
impl PartialEq for Vertices<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl fmt::Debug for Vertices<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for &Vertices<'a> {
    type Item = [f64; 3];
    type IntoIter = VertexIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Every point of a [`Vertices`], in wire order.
#[derive(Clone, Debug)]
pub struct VertexIter<'a> {
    inner: std::slice::Iter<'a, [u8; 24]>,
}

impl Iterator for VertexIter<'_> {
    type Item = [f64; 3];

    fn next(&mut self) -> Option<Self::Item> {
        Fields::new(self.inner.next()?).read_f64x3(0)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl DoubleEndedIterator for VertexIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        Fields::new(self.inner.next_back()?).read_f64x3(0)
    }
}

impl FusedIterator for VertexIter<'_> {}

/// A run of `f64` scalars, read one at a time out of the batch buffer.
#[derive(Clone, Copy)]
pub struct Scalars<'a> {
    bytes: &'a [u8],
}

/// One bulge per vertex of a [`Polyline`], or none at all.
pub type Bulges<'a> = Scalars<'a>;

/// A [`Spline`]'s knot vector.
pub type Knots<'a> = Scalars<'a>;

/// One weight per control point of a [`Spline`], or none at all.
pub type Weights<'a> = Scalars<'a>;

impl<'a> Scalars<'a> {
    const STRIDE: usize = 8;

    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// How many scalars there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len() / Self::STRIDE
    }

    /// Whether there are no scalars at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The scalar at `index`, or [`None`] past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<f64> {
        let at = index.checked_mul(Self::STRIDE)?;
        Fields::new(self.bytes).read_f64(at)
    }

    /// Every scalar, in wire order.
    #[must_use]
    pub fn iter(&self) -> ScalarIter<'a> {
        let (chunks, _) = self.bytes.as_chunks::<8>();
        ScalarIter {
            inner: chunks.iter(),
        }
    }

    /// The underlying bytes, for a caller that wants to do its own reads.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Element by element, for the same reason [`Vertices`] is.
impl PartialEq for Scalars<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl fmt::Debug for Scalars<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for &Scalars<'a> {
    type Item = f64;
    type IntoIter = ScalarIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Every scalar of a [`Scalars`], in wire order.
#[derive(Clone, Debug)]
pub struct ScalarIter<'a> {
    inner: std::slice::Iter<'a, [u8; 8]>,
}

impl Iterator for ScalarIter<'_> {
    type Item = f64;

    fn next(&mut self) -> Option<Self::Item> {
        Some(f64::from_le_bytes(*self.inner.next()?))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl DoubleEndedIterator for ScalarIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        Some(f64::from_le_bytes(*self.inner.next_back()?))
    }
}

impl FusedIterator for ScalarIter<'_> {}

// ---------------------------------------------------------------------------
// Record payloads
// ---------------------------------------------------------------------------

/// The sixteen bytes every geometry record opens with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prologue {
    /// The backing file's handle for whatever produced this record, or 0.
    pub item_handle: u64,
    /// Bit 0 is set when the record came from expanding a nested insertion.
    /// Every other bit is carried verbatim rather than masked off.
    pub flags: u32,
}

impl Prologue {
    /// Bit 0 of [`Prologue::flags`].
    pub const FROM_EXPANDED_INSERT: u32 = 1;

    /// Whether this record came from expanding a nested insertion.
    #[must_use]
    pub const fn from_expanded_insert(&self) -> bool {
        self.flags & Self::FROM_EXPANDED_INSERT != 0
    }
}

/// A view's bounding box, when it has a usable one.
///
/// There is no way to build one of these that is not a rectangle:
/// [`ViewBegin::bounds`] is the only thing that makes one, and it refuses
/// every non-finite or wrong-way-round extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Smallest x.
    pub min_x: f64,
    /// Smallest y.
    pub min_y: f64,
    /// Largest x.
    pub max_x: f64,
    /// Largest y.
    pub max_y: f64,
}

/// How many views the document has, and the code it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentBegin {
    /// Views in the document.
    pub view_count: u32,
    /// The numeric AC10xx code, or 0 when it is not known.
    pub drawing_version: u32,
}

/// One view's index, kind, extents, item count and name.
///
/// The bounding box is [`ViewBegin::bounds`] rather than a field, because it
/// is a pure function of `extents` and storing both lets a caller build a
/// `ViewBegin` whose box contradicts its own extents. Computing it also drops
/// the struct from 104 bytes to 64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBegin<'a> {
    /// Which view this is.
    pub view_index: u32,
    /// 0 model, 1 layout, 2 unknown. Left as a number on purpose: a value the
    /// native side adds later must stay representable.
    pub kind: u32,
    /// The four extent values exactly as the wire carries them, in
    /// `[min_x, min_y, max_x, max_y]` order, the inverted box included. These
    /// are exempt from the finiteness rule.
    pub extents: [f64; 4],
    /// Roughly how many items the view holds.
    pub item_count: u64,
    /// The view's name. Not terminated on the wire.
    pub name: &'a str,
}

impl ViewBegin<'_> {
    /// The extents as a rectangle, or [`None`] when they are not one.
    ///
    /// A producer with no usable extents writes the inverted box, `1e20` in
    /// both minima and `-1e20` in both maxima, and WIRE.md names `min_x >
    /// max_x` as the comparison that reads it. That comparison is necessary
    /// and not sufficient: `NaN > NaN` is false, so a `NaN` extent walks
    /// straight past it and comes out as a box whose job was to say whether it
    /// could be used. So this asks for the whole rectangle: four finite
    /// numbers, `min_x <= max_x` and `min_y <= max_y`. Anything else is
    /// [`None`], and [`ViewBegin::extents`] still carries the bytes verbatim
    /// for a caller that wants to see what was there.
    #[must_use]
    pub fn bounds(&self) -> Option<Bounds> {
        let [min_x, min_y, max_x, max_y] = self.extents;
        if !self.extents.iter().all(|v| v.is_finite()) {
            return None;
        }
        if min_x > max_x || min_y > max_y {
            return None;
        }
        Some(Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }
}

// The whole point of computing the box: the struct is the wire's own fields
// and nothing else. It was 104 bytes with an `Option<Bounds>` stored beside
// the extents it is a function of.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(
    size_of::<ViewBegin<'_>>() == 64,
    "ViewBegin is the wire's fields and nothing derived from them"
);

/// Two endpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// Handle and flags.
    pub prologue: Prologue,
    /// The first endpoint.
    pub start: [f64; 3],
    /// The second endpoint.
    pub end: [f64; 3],
}

/// A vertex run, open or closed, with a normal and a bulge per vertex.
///
/// A [`Record::Polygon`] carries this same payload with `closed` set, because
/// one layout means one reader serves both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Polyline<'a> {
    /// Handle and flags.
    pub prologue: Prologue,
    /// Whether the last vertex joins back to the first.
    pub closed: bool,
    /// The entity's normal.
    pub normal: [f64; 3],
    /// The vertices.
    pub vertices: Vertices<'a>,
    /// One bulge per vertex, or empty when every span is straight. `bulge[i]`
    /// is `tan(theta / 4)` for the span from vertex `i` to vertex `i + 1`,
    /// positive counter-clockwise about the normal.
    pub bulges: Bulges<'a>,
}

/// Centre, radius, start and end angle, and a normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// Handle and flags.
    pub prologue: Prologue,
    /// The centre.
    pub centre: [f64; 3],
    /// The radius.
    pub radius: f64,
    /// Radians, counter-clockwise, in the plane the normal defines.
    pub start_angle: f64,
    /// Radians, counter-clockwise, in the plane the normal defines.
    pub end_angle: f64,
    /// The entity's normal.
    pub normal: [f64; 3],
}

/// Centre, radius, and a normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    /// Handle and flags.
    pub prologue: Prologue,
    /// The centre.
    pub centre: [f64; 3],
    /// The radius.
    pub radius: f64,
    /// The entity's normal.
    pub normal: [f64; 3],
}

/// Centre, major axis, ratio, parameter range, and a normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    /// Handle and flags.
    pub prologue: Prologue,
    /// The centre.
    pub centre: [f64; 3],
    /// The vector from the centre to the end of the major axis.
    pub major_axis: [f64; 3],
    /// Minor over major.
    pub ratio: f64,
    /// Start of the parameter range.
    pub start_param: f64,
    /// End of the parameter range.
    pub end_param: f64,
    /// The entity's normal.
    pub normal: [f64; 3],
}

/// Degree, knots, control points and weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spline<'a> {
    /// Handle and flags.
    pub prologue: Prologue,
    /// The curve's degree. The wire does not tie it to the knot count and
    /// neither does this module.
    pub degree: u32,
    /// Bit 0 closed, bit 1 rational, bit 2 periodic. Carried verbatim.
    pub flags: u32,
    /// The knot vector.
    pub knots: Knots<'a>,
    /// The control points.
    pub controls: Vertices<'a>,
    /// One weight per control point, or empty.
    pub weights: Weights<'a>,
}

/// Position, height, rotation, and the drawing's own text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Text<'a> {
    /// Handle and flags.
    pub prologue: Prologue,
    /// Where the text sits.
    pub position: [f64; 3],
    /// Its height.
    pub height: f64,
    /// Its rotation in radians.
    pub rotation: f64,
    /// The text. Not terminated on the wire.
    pub text: &'a str,
}

/// Something the producer had to say about the drawing.
///
/// A warning is not an error: a decode that emits a hundred of them and
/// succeeds succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warning<'a> {
    /// What a consumer branches on. 1 to 999 are VIPRS allocated, 1000 and up
    /// belong to the backing source, and a code this build does not know is
    /// counted and carried on from rather than refused. Zero is not a code.
    pub code: u32,
    /// The entity the warning is about, or 0 when it is about the document.
    pub item_handle: u64,
    /// For a person reading a log. Nothing branches on it.
    pub message: &'a str,
}

/// The view index this closes, and how many records it contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewEnd {
    /// Which view this closes.
    pub view_index: u32,
    /// Records emitted for the view, its own `ViewBegin` and `ViewEnd`
    /// included. This is the only completeness proof a caller gets.
    pub record_count: u64,
}

/// Totals for the whole stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentEnd {
    /// Every record in the stream, `DocumentBegin` and this one included.
    pub total_records: u64,
    /// How many `Warning` records went past.
    pub warning_count: u64,
}

/// A record type this build does not know, skipped by its `length`.
///
/// This is what forward compatibility looks like from the inside: a later wire
/// version adds a record type and a consumer compiled against this one walks
/// straight past it. The library emits one on purpose, at
/// [`record_type::FORWARD_PROBE_MIN`], with a length matching no other record,
/// so a consumer that skipped by a size table fails visibly on a real stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unknown<'a> {
    /// The number the record declared.
    pub record_type: u16,
    /// Its payload, header excluded.
    pub payload: &'a [u8],
}

/// One record out of a batch.
///
/// `#[non_exhaustive]` because a later wire version will add types, and a
/// consumer that has to be recompiled for that is a consumer that breaks.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Record<'a> {
    /// Type 1.
    DocumentBegin(DocumentBegin),
    /// Type 2.
    ViewBegin(ViewBegin<'a>),
    /// Type 3.
    Line(Line),
    /// Type 4.
    Polyline(Polyline<'a>),
    /// Type 5.
    Arc(Arc),
    /// Type 6.
    Circle(Circle),
    /// Type 7.
    Ellipse(Ellipse),
    /// Type 8.
    Spline(Spline<'a>),
    /// Type 9, carrying type 4's payload with `closed` set.
    Polygon(Polyline<'a>),
    /// Type 10.
    Text(Text<'a>),
    /// Type 11.
    Warning(Warning<'a>),
    /// Type 12.
    ViewEnd(ViewEnd),
    /// Type 13.
    DocumentEnd(DocumentEnd),
    /// Anything else, skipped by its length.
    Unknown(Unknown<'a>),
}

impl Record<'_> {
    /// The wire number this record carries.
    ///
    /// [`Record`] is `#[non_exhaustive]`, so every downstream match needs a
    /// `_` arm, and without this that arm gets nothing at all: no number, no
    /// payload, no way to log what went past. [`Record::Unknown`] carries its
    /// own type but only covers types this build does not know, which is the
    /// opposite half of the problem.
    ///
    /// ```
    /// # use acadsharp_rs::batch::{BatchReader, Record, record_type};
    /// # let mut bytes = Vec::new();
    /// # bytes.extend_from_slice(b"VACB");
    /// # bytes.extend_from_slice(&2u16.to_le_bytes());
    /// # bytes.extend_from_slice(&1u16.to_le_bytes());
    /// # bytes.extend_from_slice(&24u32.to_le_bytes());
    /// # bytes.extend_from_slice(&1u16.to_le_bytes());
    /// # bytes.extend_from_slice(&0u16.to_le_bytes());
    /// # bytes.extend_from_slice(&24u32.to_le_bytes());
    /// # bytes.extend_from_slice(&3u32.to_le_bytes());
    /// # bytes.extend_from_slice(&1032u32.to_le_bytes());
    /// # bytes.extend_from_slice(&0u64.to_le_bytes());
    /// # let batch = BatchReader::new(&bytes)?;
    /// # let record = batch.records().next().unwrap()?;
    /// match record {
    ///     Record::Line(_) => {}
    ///     other => assert_eq!(other.record_type(), record_type::DOCUMENT_BEGIN),
    /// }
    /// # Ok::<(), acadsharp_rs::batch::BatchError>(())
    /// ```
    #[must_use]
    pub const fn record_type(&self) -> u16 {
        use record_type as t;

        match self {
            Self::DocumentBegin(_) => t::DOCUMENT_BEGIN,
            Self::ViewBegin(_) => t::VIEW_BEGIN,
            Self::Line(_) => t::LINE,
            Self::Polyline(_) => t::POLYLINE,
            Self::Arc(_) => t::ARC,
            Self::Circle(_) => t::CIRCLE,
            Self::Ellipse(_) => t::ELLIPSE,
            Self::Spline(_) => t::SPLINE,
            Self::Polygon(_) => t::POLYGON,
            Self::Text(_) => t::TEXT,
            Self::Warning(_) => t::WARNING,
            Self::ViewEnd(_) => t::VIEW_END,
            Self::DocumentEnd(_) => t::DOCUMENT_END,
            Self::Unknown(u) => u.record_type,
        }
    }
}

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

/// One batch, checked down to its last length field.
///
/// [`BatchReader::new`] validates the twelve byte header and nothing else;
/// record rules are enforced as [`BatchReader::records`] walks.
#[derive(Clone, Copy)]
pub struct BatchReader<'a> {
    payload: &'a [u8],
    flags: u16,
}

impl<'a> BatchReader<'a> {
    /// Reads a batch header off the front of `bytes`.
    ///
    /// `bytes` may be longer than the batch, which is what a stream of batches
    /// back to back looks like. It may not be shorter.
    ///
    /// # Errors
    ///
    /// [`BatchError::AbiMismatch`] when the `wire_version` is not
    /// [`WIRE_VERSION`], and [`BatchError::CorruptInput`] when the buffer is
    /// too short, the magic is wrong, or `payload_length` runs past the end.
    pub fn new(bytes: &'a [u8]) -> Result<Self, BatchError> {
        let f = Fields::new(bytes);
        let magic = f
            .array::<4>(0)
            .ok_or(BatchError::corrupt(0, Reason::ShortBatchHeader))?;
        if bytes.len() < BATCH_HEADER_LEN {
            return Err(BatchError::corrupt(0, Reason::ShortBatchHeader));
        }
        if magic != MAGIC {
            return Err(BatchError::corrupt(0, Reason::BadMagic));
        }
        let wire_version = f
            .read_u16(4)
            .ok_or(BatchError::corrupt(0, Reason::ShortBatchHeader))?;
        if wire_version != WIRE_VERSION {
            return Err(BatchError::AbiMismatch {
                offset: 4,
                found: wire_version,
                expected: WIRE_VERSION,
            });
        }
        let flags = f
            .read_u16(6)
            .ok_or(BatchError::corrupt(0, Reason::ShortBatchHeader))?;
        let payload_length = u64::from(
            f.read_u32(8)
                .ok_or(BatchError::corrupt(0, Reason::ShortBatchHeader))?,
        );
        // In u64, so a payload_length of u32::MAX cannot wrap the addition on
        // a host where usize is 32 bits.
        if BATCH_HEADER_LEN as u64 + payload_length > bytes.len() as u64 {
            return Err(BatchError::corrupt(8, Reason::PayloadPastBuffer));
        }
        let payload = usize::try_from(payload_length)
            .ok()
            .and_then(|n| f.read_slice(BATCH_HEADER_LEN, n))
            .ok_or(BatchError::corrupt(8, Reason::PayloadPastBuffer))?;
        Ok(Self { payload, flags })
    }

    /// Whether bit 0 of the batch's `flags` is set, marking the last batch of a
    /// stream.
    #[must_use]
    pub const fn is_last(&self) -> bool {
        self.flags & FLAG_LAST_BATCH != 0
    }

    /// The batch's `flags` verbatim, unknown bits included.
    #[must_use]
    pub const fn flags(&self) -> u16 {
        self.flags
    }

    /// Bytes of records in this batch, the twelve byte header excluded.
    #[must_use]
    pub const fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Bytes this batch occupies, so a caller walking a stream advances by it.
    #[must_use]
    pub const fn total_len(&self) -> usize {
        BATCH_HEADER_LEN + self.payload.len()
    }

    /// The record bytes, for a caller that wants to do its own walk.
    #[must_use]
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Every record in the batch, in wire order.
    ///
    /// The items borrow the batch buffer rather than the iterator, so nothing
    /// is copied out of it and the borrow checker is what stops a caller
    /// refilling that buffer while a record is still alive.
    #[must_use]
    pub fn records(&self) -> Records<'a> {
        self.walk_from(0)
    }

    /// Every record from `offset` on, where `offset` is one a
    /// [`Records::offset`] handed out earlier for this same batch.
    ///
    /// This is what a caller needs to stop in the middle of a batch and pick
    /// the walk up later without either re-walking from the first record (a
    /// batch of 64 KiB of `Line` records is about 900 of them, so resuming
    /// once per record is 400,000 steps instead of 900) or writing a second
    /// copy of the framing rules.
    ///
    /// # Errors
    ///
    /// [`ResumeError`] when the offset is inside the batch header, past the
    /// end of the batch, or not a multiple of four past the header. Those are
    /// the checks that are free. An offset that is aligned and inside the
    /// batch but lands in the middle of a record is **not** caught: finding
    /// that out means walking from the first record, which is the work this
    /// function exists to skip. What comes out of a walk started there is
    /// still bounded and still refused or read, it is just not the records the
    /// producer wrote, so pass an offset this batch gave you.
    ///
    /// ```
    /// # use acadsharp_rs::batch::{BatchReader, Record};
    /// # let mut bytes = Vec::new();
    /// # bytes.extend_from_slice(b"VACB");
    /// # bytes.extend_from_slice(&2u16.to_le_bytes());
    /// # bytes.extend_from_slice(&1u16.to_le_bytes());
    /// # bytes.extend_from_slice(&48u32.to_le_bytes());
    /// # for view_count in [3u32, 4] {
    /// #     bytes.extend_from_slice(&1u16.to_le_bytes());
    /// #     bytes.extend_from_slice(&0u16.to_le_bytes());
    /// #     bytes.extend_from_slice(&24u32.to_le_bytes());
    /// #     bytes.extend_from_slice(&view_count.to_le_bytes());
    /// #     bytes.extend_from_slice(&1032u32.to_le_bytes());
    /// #     bytes.extend_from_slice(&0u64.to_le_bytes());
    /// # }
    /// let batch = BatchReader::new(&bytes)?;
    /// let mut records = batch.records();
    /// records.next();
    /// let saved = records.offset();
    ///
    /// let resumed: Vec<_> = batch.records_from(saved)?.collect();
    /// let straight_through: Vec<_> = records.collect();
    /// assert_eq!(resumed, straight_through);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn records_from(&self, offset: u64) -> Result<Records<'a>, ResumeError> {
        let header = BATCH_HEADER_LEN as u64;
        if offset < header {
            return Err(ResumeError::BeforeFirstRecord {
                offset,
                first_record: header,
            });
        }
        let total_len = self.total_len() as u64;
        if offset > total_len {
            return Err(ResumeError::PastEndOfBatch { offset, total_len });
        }
        let pos = offset - header;
        if !pos.is_multiple_of(4) {
            return Err(ResumeError::NotAligned { offset });
        }
        // `pos <= payload.len()` from the bound above, so this fits a usize.
        Ok(self.walk_from(pos as usize))
    }

    fn walk_from(&self, pos: usize) -> Records<'a> {
        Records {
            payload: self.payload,
            pos,
            // One per smallest possible record left, plus one. A record shorter
            // than its own header is already refused, so this bound is what
            // turns a mistake in that rule into a failed parse rather than a
            // hang. It counts from `pos` so resuming does not hand out a budget
            // for records already walked.
            budget: (self.payload.len() - pos) as u64 / RECORD_HEADER_LEN + 1,
            stopped: false,
        }
    }
}

impl fmt::Debug for BatchReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchReader")
            .field("flags", &self.flags)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

/// Every record in one batch.
///
/// Once a record is refused the iterator is done: there is nothing to
/// resynchronise on in this format, so it yields the error once and [`None`]
/// forever after.
#[derive(Clone, Debug)]
pub struct Records<'a> {
    payload: &'a [u8],
    pos: usize,
    budget: u64,
    stopped: bool,
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, BatchError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stopped || self.pos >= self.payload.len() {
            self.stopped = true;
            return None;
        }
        match self.step() {
            Ok(record) => Some(Ok(record)),
            Err(e) => {
                self.stopped = true;
                Some(Err(e))
            }
        }
    }
}

impl FusedIterator for Records<'_> {}

impl<'a> Records<'a> {
    /// Bytes from the start of the batch to the record about to be read.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        BATCH_HEADER_LEN as u64 + self.pos as u64
    }

    /// A `Records` with its cursor and its iteration bound planted, so the two
    /// backstops that are unreachable by construction can be watched firing.
    ///
    /// There is no way to reach either from bytes, which is the point of them,
    /// and an error variant nothing ever asserts is an error variant nobody
    /// knows is wired up.
    #[cfg(test)]
    const fn planted(payload: &'a [u8], pos: usize, budget: u64) -> Self {
        Self {
            payload,
            pos,
            budget,
            stopped: false,
        }
    }

    fn step(&mut self) -> Result<Record<'a>, BatchError> {
        let at = self.offset();
        if self.budget == 0 {
            return Err(BatchError::internal(at));
        }
        self.budget -= 1;

        let rest = Fields::new(
            self.payload
                .get(self.pos..)
                .ok_or(BatchError::internal(at))?,
        );
        let remaining = rest.bytes.len() as u64;
        if remaining < RECORD_HEADER_LEN {
            return Err(BatchError::corrupt(at, Reason::ShortRecordHeader));
        }

        let kind = rest.read_u16(0).ok_or(BatchError::internal(at))?;
        let reserved = rest.read_u16(2).ok_or(BatchError::internal(at))?;
        let length = u64::from(rest.read_u32(4).ok_or(BatchError::internal(at))?);

        // The four framing rules, in the order WIRE.md states them, then the
        // reserved rule. Rule 2 is what keeps the cursor moving.
        if length < RECORD_HEADER_LEN {
            return Err(BatchError::corrupt(at, Reason::LengthBelowHeader));
        }
        if length % 4 != 0 {
            return Err(BatchError::corrupt(at, Reason::LengthNotMultipleOfFour));
        }
        if length > remaining {
            return Err(BatchError::corrupt(at, Reason::LengthPastPayload));
        }
        if reserved != 0 {
            return Err(BatchError::corrupt(at, Reason::RecordReservedNotZero));
        }

        // Every conversion below is bounded by `length <= remaining`, which is
        // bounded by the payload slice, so it fits a usize on any host.
        let record_len = usize::try_from(length).map_err(|_| BatchError::internal(at))?;
        let payload_len = record_len - (RECORD_HEADER_LEN as usize);
        let payload = Fields::new(
            rest.read_slice(RECORD_HEADER_LEN as usize, payload_len)
                .ok_or(BatchError::internal(at))?,
        );
        self.pos += record_len;

        decode(kind, length, payload, at)
    }
}

// ---------------------------------------------------------------------------
// Payload decoding
// ---------------------------------------------------------------------------

fn decode<'a>(kind: u16, length: u64, p: Fields<'a>, at: u64) -> Result<Record<'a>, BatchError> {
    use record_type as t;

    match kind {
        t::DOCUMENT_BEGIN => {
            exact(length, LEN_DOCUMENT_BEGIN, at)?;
            Ok(Record::DocumentBegin(DocumentBegin {
                view_count: u32_at(p, 0, at)?,
                drawing_version: u32_at(p, 4, at)?,
            }))
        }
        t::VIEW_BEGIN => decode_view_begin(p, length, at).map(Record::ViewBegin),
        t::LINE => {
            exact(length, LEN_LINE, at)?;
            let prologue = prologue(p, at)?;
            finite(p, 16, at)?;
            Ok(Record::Line(Line {
                prologue,
                start: f64x3_at(p, 16, at)?,
                end: f64x3_at(p, 40, at)?,
            }))
        }
        t::POLYLINE => decode_polyline(p, length, at).map(Record::Polyline),
        t::ARC => {
            exact(length, LEN_ARC, at)?;
            let prologue = prologue(p, at)?;
            finite(p, 16, at)?;
            Ok(Record::Arc(Arc {
                prologue,
                centre: f64x3_at(p, 16, at)?,
                radius: f64_at(p, 40, at)?,
                start_angle: f64_at(p, 48, at)?,
                end_angle: f64_at(p, 56, at)?,
                normal: f64x3_at(p, 64, at)?,
            }))
        }
        t::CIRCLE => {
            exact(length, LEN_CIRCLE, at)?;
            let prologue = prologue(p, at)?;
            finite(p, 16, at)?;
            Ok(Record::Circle(Circle {
                prologue,
                centre: f64x3_at(p, 16, at)?,
                radius: f64_at(p, 40, at)?,
                normal: f64x3_at(p, 48, at)?,
            }))
        }
        t::ELLIPSE => {
            exact(length, LEN_ELLIPSE, at)?;
            let prologue = prologue(p, at)?;
            finite(p, 16, at)?;
            Ok(Record::Ellipse(Ellipse {
                prologue,
                centre: f64x3_at(p, 16, at)?,
                major_axis: f64x3_at(p, 40, at)?,
                ratio: f64_at(p, 64, at)?,
                start_param: f64_at(p, 72, at)?,
                end_param: f64_at(p, 80, at)?,
                normal: f64x3_at(p, 88, at)?,
            }))
        }
        t::SPLINE => decode_spline(p, length, at).map(Record::Spline),
        t::POLYGON => decode_polyline(p, length, at).map(Record::Polygon),
        t::TEXT => decode_text(p, length, at).map(Record::Text),
        t::WARNING => decode_warning(p, length, at).map(Record::Warning),
        t::VIEW_END => {
            exact(length, LEN_VIEW_END, at)?;
            Ok(Record::ViewEnd(ViewEnd {
                view_index: u32_at(p, 0, at)?,
                record_count: u64_at(p, 8, at)?,
            }))
        }
        t::DOCUMENT_END => {
            exact(length, LEN_DOCUMENT_END, at)?;
            Ok(Record::DocumentEnd(DocumentEnd {
                total_records: u64_at(p, 0, at)?,
                warning_count: u64_at(p, 8, at)?,
            }))
        }
        // Skipped by `length`, never by a size table. The cursor has already
        // moved, so parsing carries on with the record after this one.
        other => Ok(Record::Unknown(Unknown {
            record_type: other,
            payload: p.bytes,
        })),
    }
}

fn decode_view_begin<'a>(p: Fields<'a>, length: u64, at: u64) -> Result<ViewBegin<'a>, BatchError> {
    if length < VIEW_BEGIN_FIXED {
        return Err(BatchError::corrupt(at, Reason::WrongFixedLength));
    }
    let name_len = u64::from(u32_at(p, 48, at)?);
    if VIEW_BEGIN_FIXED + pad4(name_len) != length {
        return Err(BatchError::corrupt(at, Reason::StringLengthMismatch));
    }
    // Extents are exempt from the finiteness rule on purpose: they are a box
    // the producer reports rather than a shape anybody draws, and a view
    // holding nothing has no finite one. Whether they are a rectangle is
    // `ViewBegin::bounds`'s question, not this one's.
    let extents = [
        f64_at(p, 8, at)?,
        f64_at(p, 16, at)?,
        f64_at(p, 24, at)?,
        f64_at(p, 32, at)?,
    ];
    Ok(ViewBegin {
        view_index: u32_at(p, 0, at)?,
        kind: u32_at(p, 4, at)?,
        extents,
        item_count: u64_at(p, 40, at)?,
        name: string_at(p, 56, name_len, at)?,
    })
}

fn decode_polyline<'a>(p: Fields<'a>, length: u64, at: u64) -> Result<Polyline<'a>, BatchError> {
    if length < POLYLINE_FIXED {
        return Err(BatchError::corrupt(at, Reason::WrongFixedLength));
    }
    let point_count = u64::from(u32_at(p, 16, at)?);
    let closed = u32_at(p, 20, at)?;
    let bulge_count = u64::from(u32_at(p, 24, at)?);
    if bulge_count != 0 && bulge_count != point_count {
        return Err(BatchError::corrupt(at, Reason::BulgeCount));
    }
    // The identity, in u64. In u32 this is where a 72 byte record gets to
    // claim 68 GB of vertices.
    if polyline_length(point_count, bulge_count) != length {
        return Err(BatchError::corrupt(at, Reason::PolylineLengthMismatch));
    }
    // Everything from the normal to the end of the bulges, which is the whole
    // rest of the payload.
    finite(p, 32, at)?;
    let vertex_bytes = bytes_for(24 * point_count, at)?;
    let bulge_bytes = bytes_for(8 * bulge_count, at)?;
    let vertices = p
        .read_slice(56, vertex_bytes)
        .ok_or(BatchError::internal(at))?;
    // 56 + vertex_bytes cannot overflow: the read above proves it is inside a
    // slice that already fits a usize.
    let bulges = p
        .read_slice(56 + vertex_bytes, bulge_bytes)
        .ok_or(BatchError::internal(at))?;
    Ok(Polyline {
        prologue: prologue(p, at)?,
        closed: closed != 0,
        normal: f64x3_at(p, 32, at)?,
        vertices: Vertices::new(vertices),
        bulges: Scalars::new(bulges),
    })
}

fn decode_spline<'a>(p: Fields<'a>, length: u64, at: u64) -> Result<Spline<'a>, BatchError> {
    if length < SPLINE_FIXED {
        return Err(BatchError::corrupt(at, Reason::WrongFixedLength));
    }
    let knot_count = u64::from(u32_at(p, 24, at)?);
    let control_count = u64::from(u32_at(p, 28, at)?);
    let weight_count = u64::from(u32_at(p, 32, at)?);
    if weight_count != 0 && weight_count != control_count {
        return Err(BatchError::corrupt(at, Reason::WeightCount));
    }
    if spline_length(knot_count, control_count, weight_count) != length {
        return Err(BatchError::corrupt(at, Reason::SplineLengthMismatch));
    }
    finite(p, 40, at)?;
    let knot_bytes = bytes_for(8 * knot_count, at)?;
    let control_bytes = bytes_for(24 * control_count, at)?;
    let weight_bytes = bytes_for(8 * weight_count, at)?;
    let knots = p
        .read_slice(40, knot_bytes)
        .ok_or(BatchError::internal(at))?;
    let controls = p
        .read_slice(40 + knot_bytes, control_bytes)
        .ok_or(BatchError::internal(at))?;
    let weights = p
        .read_slice(40 + knot_bytes + control_bytes, weight_bytes)
        .ok_or(BatchError::internal(at))?;
    Ok(Spline {
        prologue: prologue(p, at)?,
        degree: u32_at(p, 16, at)?,
        flags: u32_at(p, 20, at)?,
        knots: Scalars::new(knots),
        controls: Vertices::new(controls),
        weights: Scalars::new(weights),
    })
}

fn decode_text<'a>(p: Fields<'a>, length: u64, at: u64) -> Result<Text<'a>, BatchError> {
    if length < TEXT_FIXED {
        return Err(BatchError::corrupt(at, Reason::WrongFixedLength));
    }
    let byte_len = u64::from(u32_at(p, 56, at)?);
    if TEXT_FIXED + pad4(byte_len) != length {
        return Err(BatchError::corrupt(at, Reason::StringLengthMismatch));
    }
    // The scan stops at 56. Past that are the drawing's own bytes, and reading
    // those as f64s would refuse a perfectly good text record for nothing.
    let numbers = p.read_slice(16, 40).ok_or(BatchError::internal(at))?;
    if !all_finite(numbers) {
        return Err(BatchError::corrupt(at, Reason::NonFiniteFloat));
    }
    Ok(Text {
        prologue: prologue(p, at)?,
        position: f64x3_at(p, 16, at)?,
        height: f64_at(p, 40, at)?,
        rotation: f64_at(p, 48, at)?,
        text: string_at(p, 64, byte_len, at)?,
    })
}

fn decode_warning<'a>(p: Fields<'a>, length: u64, at: u64) -> Result<Warning<'a>, BatchError> {
    if length < WARNING_FIXED {
        return Err(BatchError::corrupt(at, Reason::WrongFixedLength));
    }
    let message_len = u64::from(u32_at(p, 16, at)?);
    if WARNING_FIXED + pad4(message_len) != length {
        return Err(BatchError::corrupt(at, Reason::StringLengthMismatch));
    }
    let code = u32_at(p, 0, at)?;
    if code == 0 {
        return Err(BatchError::corrupt(at, Reason::ZeroWarningCode));
    }
    // A code this build does not know is counted and carried on from, never
    // refused. Adding a warning code is not a wire version bump.
    Ok(Warning {
        code,
        item_handle: u64_at(p, 8, at)?,
        message: string_at(p, 24, message_len, at)?,
    })
}

// --- the small helpers every decoder shares --------------------------------

/// A fixed size record has exactly one legal length.
fn exact(length: u64, want: u64, at: u64) -> Result<(), BatchError> {
    if length == want {
        Ok(())
    } else {
        Err(BatchError::corrupt(at, Reason::WrongFixedLength))
    }
}

/// The sixteen byte geometry prologue.
fn prologue(p: Fields<'_>, at: u64) -> Result<Prologue, BatchError> {
    Ok(Prologue {
        item_handle: u64_at(p, 0, at)?,
        flags: u32_at(p, 8, at)?,
    })
}

/// Every `f64` from `from` to the end of the payload is finite.
fn finite(p: Fields<'_>, from: usize, at: u64) -> Result<(), BatchError> {
    let tail = p.tail(from).ok_or(BatchError::internal(at))?;
    if all_finite(tail) {
        Ok(())
    } else {
        Err(BatchError::corrupt(at, Reason::NonFiniteFloat))
    }
}

/// A byte count computed in `u64`, brought down to a `usize` once.
fn bytes_for(count: u64, at: u64) -> Result<usize, BatchError> {
    usize::try_from(count).map_err(|_| BatchError::internal(at))
}

fn u32_at(p: Fields<'_>, offset: usize, at: u64) -> Result<u32, BatchError> {
    p.read_u32(offset).ok_or(BatchError::internal(at))
}

fn u64_at(p: Fields<'_>, offset: usize, at: u64) -> Result<u64, BatchError> {
    p.read_u64(offset).ok_or(BatchError::internal(at))
}

fn f64_at(p: Fields<'_>, offset: usize, at: u64) -> Result<f64, BatchError> {
    p.read_f64(offset).ok_or(BatchError::internal(at))
}

fn f64x3_at(p: Fields<'_>, offset: usize, at: u64) -> Result<[f64; 3], BatchError> {
    p.read_f64x3(offset).ok_or(BatchError::internal(at))
}

fn string_at<'a>(p: Fields<'a>, offset: usize, len: u64, at: u64) -> Result<&'a str, BatchError> {
    let len = bytes_for(len, at)?;
    let bytes = p.read_slice(offset, len).ok_or(BatchError::internal(at))?;
    // The padding past `len` is not checked and not refused: it is not the
    // string, and a producer is free to put whatever it likes there.
    std::str::from_utf8(bytes).map_err(|_| BatchError::corrupt(at, Reason::InvalidUtf8))
}

// ---------------------------------------------------------------------------
// The two backstops, watched firing
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// One well formed `DocumentBegin`, with no batch header in front of it,
    /// which is what `Records` walks.
    fn one_record_payload() -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&record_type::DOCUMENT_BEGIN.to_le_bytes());
        p.extend_from_slice(&0u16.to_le_bytes());
        p.extend_from_slice(&24u32.to_le_bytes());
        p.extend_from_slice(&3u32.to_le_bytes());
        p.extend_from_slice(&1032u32.to_le_bytes());
        p.extend_from_slice(&0u64.to_le_bytes());
        assert_eq!(p.len(), 24);
        p
    }

    #[test]
    fn a_spent_iteration_bound_is_an_internal_error() {
        // The same bytes read fine with a budget, so the only thing this test
        // changes is the backstop.
        let payload = one_record_payload();
        let mut healthy = Records::planted(&payload, 0, 1);
        assert!(matches!(healthy.next(), Some(Ok(Record::DocumentBegin(_)))));

        let mut spent = Records::planted(&payload, 0, 0);
        assert_eq!(
            spent.next(),
            Some(Err(BatchError::Internal { offset: 12 })),
            "a record that did not advance the cursor is my bug, not corrupt input"
        );
        assert!(spent.next().is_none(), "and the walk stops there");
    }

    #[test]
    fn a_cursor_past_the_payload_is_an_internal_error() {
        // The other backstop. `step` cannot be reached this way from bytes,
        // which is exactly why it needs planting to be watched at all.
        let payload = one_record_payload();
        let past = payload.len() + 1;
        let mut wrong = Records::planted(&payload, past, 16);
        assert_eq!(
            wrong.step(),
            Err(BatchError::Internal {
                offset: 12 + past as u64
            })
        );
    }

    #[test]
    fn an_internal_error_says_whose_bug_it_is() {
        let e = BatchError::Internal { offset: 44 };
        assert_eq!(e.offset(), 44);
        let rendered = e.to_string();
        assert!(rendered.contains("offset 44"), "{rendered}");
        assert!(rendered.contains("bug in acadsharp-rs"), "{rendered}");
    }
}
