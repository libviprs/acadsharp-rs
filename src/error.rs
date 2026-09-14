//! One error type for everything the safe API can refuse, and the one place a
//! native result code turns into it.
//!
//! Three rules shaped this.
//!
//! There is no error string anywhere on the C boundary, in either direction,
//! so a caller switches on a number and never parses text. That number has to
//! arrive here as something with a name, and a number nothing declares has to
//! arrive as [`Error::Native`] rather than as a panic or a silent success.
//!
//! There is no [`std::io::Error`] in here, which the issue's sketch had.
//! Opening a path hands the path's bytes to the library and never reads the
//! file in Rust, so there is no `io` call to fail. The failure this crate
//! really has on that route is a path whose bytes are not UTF-8, and
//! [`Error::PathNotUtf8`] is it.
//!
//! `VIPRS_ACAD_BUFFER_TOO_SMALL` is not in here as a variant. It is about a
//! buffer this crate owns and says nothing about the decode, so a caller who
//! saw it could do nothing with it. [`crate::PrimitiveStream`] grows and
//! retries; a refusal it cannot answer is [`Error::BatchTooLarge`], which
//! names both numbers, or [`Error::Internal`] if the library breaks its own
//! contract.

use core::fmt;

use crate::abi::HeaderMismatch;
use crate::batch::BatchError;
use crate::ffi;

/// The crate's result type.
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Everything the safe API can refuse.
///
/// `#[non_exhaustive]`, because the boundary's own error model is frozen by
/// number but this crate's is not: a code the library adds later deserves a
/// name here, and adding one should not break a caller who matched on the
/// names that already existed.
///
/// The three variants that carry named fields are `#[non_exhaustive]` too, for
/// the same reason one layer in. ABI.md's sentence for code 2 is "check
/// `dwg_version_min` and `dwg_version_max`", and a later revision that adds a
/// third thing to check should be a field rather than a new variant. So match
/// them with a trailing `..`, and reach them with
/// [`Error::from_native_code`] rather than by building one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A null pointer, a zero length where one is not allowed, an index out of
    /// range, or a struct the library does not recognise. Nothing was written
    /// and nothing was allocated.
    InvalidArgument,
    /// The input is a format, or a version of one, this build does not read.
    ///
    /// The range comes with the refusal because ABI.md's whole sentence for
    /// this code is "check `dwg_version_min` and `dwg_version_max`", and an
    /// error that makes the caller go back and ask is an error that gets
    /// logged without them.
    #[non_exhaustive]
    UnsupportedFormat {
        /// The oldest AC10xx drawing version this build reads.
        dwg_version_min: u32,
        /// The newest AC10xx drawing version this build reads.
        dwg_version_max: u32,
    },
    /// The input is the right format and is damaged.
    CorruptInput,
    /// The caller asked for one specific thing this build cannot produce. A
    /// drawing full of shapes the decoder has no record type for does not fail
    /// this way: it emits [`crate::Warning`] records and carries on.
    UnsupportedEntity,
    /// An allocation inside the library failed.
    OutOfMemory,
    /// The cancel flag was non-zero. Terminal for a decode: clearing the flag
    /// and asking again gets this same answer, not the rest of the drawing.
    ///
    /// Spelled with two `l`s, which is the spelling the rest of this crate
    /// uses. The C constant is `VIPRS_ACAD_CANCELED` and stays that way in
    /// the `ffi` module, where it is a transcription.
    Cancelled,
    /// A bound in [`crate::Limits`] was reached, and nothing else. Terminal
    /// for a decode.
    LimitExceeded,
    /// The library's own handshake failed, or a stream carried a wire version
    /// it does not parse. Both mean the two ends of the boundary were built
    /// against different contracts, and the remedy is to rebuild one of them.
    AbiMismatch,
    /// This crate's handshake refused the library it linked, which is the
    /// same failure seen from this side: the vendored header and the library
    /// came from different commits.
    HeaderMismatch(HeaderMismatch),
    /// A batch of the decode stream would not parse.
    ///
    /// [`std::error::Error::source`] hands back the [`BatchError`], which
    /// carries the offset and the rule that was broken.
    Batch(BatchError),
    /// One batch needs more room than this crate is willing to hold for it.
    ///
    /// A single legal record can approach 2^31 - 1 bytes, so the growth is
    /// capped rather than trusted. Raise the ceiling with
    /// [`crate::Decoder::with_max_batch_bytes`], or lower
    /// [`crate::Limits::with_max_polyline_points`] so the library never builds
    /// a record that big.
    #[non_exhaustive]
    BatchTooLarge {
        /// What the library asked for, in bytes.
        required: u64,
        /// The ceiling it was measured against.
        max_batch_bytes: u64,
    },
    /// A path whose bytes are not UTF-8.
    ///
    /// The boundary takes a path as UTF-8 bytes and a length, so this crate
    /// refuses one it cannot spell rather than forwarding something the
    /// library would have to guess at.
    PathNotUtf8,
    /// An empty path, or an empty byte slice. A zero length is a caller
    /// mistake everywhere on this boundary, so it is refused here rather than
    /// sent across.
    EmptyInput,
    /// This build of the crate has no native library behind it.
    ///
    /// The crate compiles, documents and tests with no archive present. The
    /// public surface does not change shape: every type and every function is
    /// still here, the calls underneath them are what is compiled out, and the
    /// first thing a caller does, [`crate::Decoder::new`], answers this. A
    /// consumer cannot write `cfg(acadsharp_linked)` themselves, so the
    /// absence has to arrive as a value. [`Error::is_unlinked`] is the ask.
    Unlinked,
    /// A result code the header does not declare.
    ///
    /// Never a panic and never a silent success: a number this crate has no
    /// name for still crosses as a number.
    Native(u32),
    /// A bug, either in the library or in this crate.
    ///
    /// The string is for a person reading a log and is not part of the
    /// contract, so match on the variant and never on the text.
    #[non_exhaustive]
    Internal {
        /// What went wrong, for a human.
        what: &'static str,
    },
}

impl Error {
    /// Maps a native result code onto this type.
    ///
    /// `VIPRS_ACAD_OK` is success and maps to [`None`]; everything else
    /// maps to a variant, and a code the header does not declare maps to
    /// [`Error::Native`].
    ///
    /// The two drawing-version numbers come from [`crate::Capabilities`] and
    /// are only used by [`Error::UnsupportedFormat`]. They are arguments
    /// rather than something this function fetches, so the one table every
    /// caller's control flow hangs off is a plain function over three
    /// integers, testable in a job that never links a library.
    ///
    /// ```
    /// use acadsharp_rs::Error;
    ///
    /// assert_eq!(Error::from_native_code(0, 1014, 1032), None);
    /// assert_eq!(Error::from_native_code(9, 1014, 1032), Some(Error::LimitExceeded));
    /// assert_eq!(Error::from_native_code(4242, 1014, 1032), Some(Error::Native(4242)));
    /// ```
    #[must_use]
    pub const fn from_native_code(
        code: u32,
        dwg_version_min: u32,
        dwg_version_max: u32,
    ) -> Option<Self> {
        let mapped = match code {
            ffi::VIPRS_ACAD_OK => return None,
            ffi::VIPRS_ACAD_INVALID_ARGUMENT => Self::InvalidArgument,
            ffi::VIPRS_ACAD_UNSUPPORTED_FORMAT => Self::UnsupportedFormat {
                dwg_version_min,
                dwg_version_max,
            },
            ffi::VIPRS_ACAD_CORRUPT_INPUT => Self::CorruptInput,
            ffi::VIPRS_ACAD_UNSUPPORTED_ENTITY => Self::UnsupportedEntity,
            ffi::VIPRS_ACAD_OUT_OF_MEMORY => Self::OutOfMemory,
            ffi::VIPRS_ACAD_CANCELED => Self::Cancelled,
            ffi::VIPRS_ACAD_INTERNAL_ERROR => Self::Internal {
                what: "the native library reported a bug of its own",
            },
            ffi::VIPRS_ACAD_ABI_MISMATCH => Self::AbiMismatch,
            ffi::VIPRS_ACAD_LIMIT_EXCEEDED => Self::LimitExceeded,
            // This one never reaches a caller through a stream: the buffer is
            // this crate's and growing it is this crate's job. Reaching it
            // here means the mapping was asked about a code the pull loop
            // should have swallowed.
            ffi::VIPRS_ACAD_BUFFER_TOO_SMALL => Self::Internal {
                what: "the library asked for a bigger buffer somewhere this crate does not grow one",
            },
            other => Self::Native(other),
        };
        Some(mapped)
    }

    /// Whether this is [`Error::Unlinked`].
    ///
    /// Worth an accessor because it is the one variant a caller may want to
    /// treat as "not an error here", and they cannot write the `cfg` that
    /// would have told them at compile time.
    #[must_use]
    pub const fn is_unlinked(&self) -> bool {
        matches!(self, Self::Unlinked)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument => f.write_str("the library refused one of the arguments"),
            Self::UnsupportedFormat {
                dwg_version_min,
                dwg_version_max,
            } => write!(
                f,
                "this build does not read that drawing format, it reads AC10xx versions \
                 {dwg_version_min} to {dwg_version_max}"
            ),
            Self::CorruptInput => f.write_str("the drawing is the right format and is damaged"),
            Self::UnsupportedEntity => {
                f.write_str("this build cannot produce the one thing that was asked for")
            }
            Self::OutOfMemory => f.write_str("an allocation inside the library failed"),
            Self::Cancelled => {
                f.write_str("the decode was cancelled, and a cancel is final for that decode")
            }
            Self::LimitExceeded => f.write_str("the decode hit a bound set in Limits"),
            Self::AbiMismatch => {
                f.write_str("the library and this consumer were built against different contracts")
            }
            Self::HeaderMismatch(inner) => write!(f, "{inner}"),
            Self::Batch(inner) => write!(f, "{inner}"),
            Self::BatchTooLarge {
                required,
                max_batch_bytes,
            } => write!(
                f,
                "one batch needs {required} bytes and this crate holds at most \
                 {max_batch_bytes}"
            ),
            Self::PathNotUtf8 => {
                f.write_str("that path is not UTF-8, and the boundary takes UTF-8 bytes")
            }
            Self::EmptyInput => f.write_str("there is nothing to open: the input is empty"),
            Self::Unlinked => f.write_str(
                "this build of acadsharp-rs has no native library behind it, so there is \
                 nothing to decode with",
            ),
            Self::Native(code) => write!(
                f,
                "the library returned result code {code}, which this build has no name for"
            ),
            Self::Internal { what } => write!(
                f,
                "{what}, which is a bug rather than anything to do with the drawing"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Batch(inner) => Some(inner),
            Self::HeaderMismatch(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<BatchError> for Error {
    fn from(inner: BatchError) -> Self {
        Self::Batch(inner)
    }
}

impl From<HeaderMismatch> for Error {
    fn from(inner: HeaderMismatch) -> Self {
        Self::HeaderMismatch(inner)
    }
}
