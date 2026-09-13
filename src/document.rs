//! The handshake, an open document, and its views.
//!
//! The shape is the boundary's, with the unsafe parts discharged:
//!
//! ```text
//! Decoder::new                 the handshake, once
//!   Document::open_path / open_bytes
//!     views, view_count, view
//!     decode / decode_with_cancel  ->  PrimitiveStream
//! ```
//!
//! # None of these cross a thread
//!
//! [`Decoder`], [`Document`] and [`crate::PrimitiveStream`] are neither
//! [`Send`] nor [`Sync`], and there is no `unsafe impl` anywhere in the crate
//! putting either back. One decode handle is single threaded, calls on it must
//! not overlap, and nothing on the boundary is re-entrant, so a handle that
//! crossed a thread boundary would be a data race the type system had waved
//! through. The absence falls out of holding a raw pointer, which is the
//! cheapest correct answer.
//!
//! ABI.md does allow two handles on two threads, including two decode handles
//! on one document, so this is liftable later behind a type that owns the
//! pairing. It is not liftable by writing `unsafe impl Send`, and
//! `tests/api_surface.rs` fails the moment anybody does.

use core::fmt;
use std::path::Path;

use crate::capabilities::Capabilities;
use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::stream::{self, PrimitiveStream};
use crate::sys;
use crate::{CancelToken, batch};

/// The handshake, done once, and the buffer policy every decode inherits.
///
/// Constructing one asks the library which contract it was built against and
/// refuses it if that is not this one: the version is the coarse check and the
/// fingerprint is the fine one, and they differ exactly when the vendored
/// header and the library came from different commits. That is the failure
/// worth catching, because the library loads, every symbol resolves, and a
/// struct field sits four bytes from where this crate believes it is.
///
/// Every later call assumes it passed, which is why [`Document::open_path`]
/// and [`Document::open_bytes`] take one.
pub struct Decoder {
    capabilities: Capabilities,
    initial_batch_bytes: usize,
    max_batch_bytes: usize,
    /// Not [`Send`] and not [`Sync`], for the reason in the module docs.
    ///
    /// This type holds no handle of its own, so the marker is deliberate
    /// rather than inherited: a `Decoder` is what every open call takes, and a
    /// caller who can send one around builds the expectation that the rest of
    /// the API travels with it. Adding `Send` later is not a breaking change,
    /// and taking it away would be.
    ///
    /// Never read, which is the point: the field is here for its type. A
    /// `PhantomData` would be exempt from the dead code lint and would also
    /// send every compile-fail case's expected output into core's `marker.rs`.
    #[allow(dead_code)]
    not_thread_safe: sys::NotThreadSafe,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoder")
            .field("capabilities", &self.capabilities)
            .field("initial_batch_bytes", &self.initial_batch_bytes)
            .field("max_batch_bytes", &self.max_batch_bytes)
            .finish()
    }
}

impl Decoder {
    /// Runs the handshake and reads what the build can do.
    ///
    /// # Errors
    ///
    /// [`Error::HeaderMismatch`] when the library and the vendored header
    /// disagree, carrying both pairs of numbers, and [`Error::Unlinked`] when
    /// this build of the crate has no library behind it at all.
    pub fn new() -> Result<Self> {
        sys::handshake()?;

        Ok(Self {
            capabilities: Capabilities::from_raw(sys::capabilities()?),
            initial_batch_bytes: stream::DEFAULT_INITIAL_BATCH_BYTES,
            max_batch_bytes: stream::DEFAULT_MAX_BATCH_BYTES,
            not_thread_safe: sys::NotThreadSafe::new(),
        })
    }

    /// What this build of the library can do.
    #[must_use]
    pub const fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Sets the size of the buffer a new stream starts with.
    ///
    /// The default is the documented batch target, so the common case never
    /// round trips. Anything below the twelve byte batch header is raised to
    /// it: a capacity in `1..=11` comes back asking for 12, which is the
    /// smallest legal batch rather than the size of the next one, and a caller
    /// who started there would spend its one retry learning that.
    #[must_use]
    pub const fn with_initial_batch_bytes(mut self, bytes: usize) -> Self {
        self.initial_batch_bytes = if bytes < batch::BATCH_HEADER_LEN {
            batch::BATCH_HEADER_LEN
        } else {
            bytes
        };
        self
    }

    /// Sets how large a single batch this crate is willing to hold.
    ///
    /// A batch is one record at least, and a record can approach 2^31 - 1
    /// bytes: a polyline at the default point bound, carrying a bulge per
    /// vertex, is 32 MB on its own. So the growth is capped and a batch past
    /// the cap is [`Error::BatchTooLarge`] rather than an allocation nobody
    /// asked for.
    #[must_use]
    pub const fn with_max_batch_bytes(mut self, bytes: usize) -> Self {
        self.max_batch_bytes = bytes;
        self
    }

    /// The size a new stream's buffer starts at.
    #[must_use]
    pub const fn initial_batch_bytes(&self) -> usize {
        self.initial_batch_bytes
    }

    /// The largest single batch this crate will hold.
    #[must_use]
    pub const fn max_batch_bytes(&self) -> usize {
        self.max_batch_bytes
    }
}

/// An open document.
///
/// Dropping it calls `viprs_acad_close`, which releases every decode handle
/// still open on it. That is why [`PrimitiveStream`] borrows the document:
/// borrowck then refuses the ordering that would have been a use after free,
/// rather than the library having to refuse it at run time.
pub struct Document {
    handle: sys::DocumentHandle,
    capabilities: Capabilities,
    initial_batch_bytes: usize,
    max_batch_bytes: usize,
}

impl fmt::Debug for Document {
    /// Everything but the handle, which is an opaque number a caller must
    /// never dereference and would learn nothing from.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl Document {
    /// Opens a drawing from a filesystem path.
    ///
    /// The path crosses as UTF-8 bytes and a length, and nothing here reads
    /// the file: reading it is the library's job, and doing it twice would
    /// mean holding a copy of every drawing in memory for no reason. Prefer
    /// this to [`Document::open_bytes`] whenever the drawing is on disk.
    ///
    /// # Errors
    ///
    /// [`Error::PathNotUtf8`] for a path this boundary cannot spell,
    /// [`Error::EmptyInput`] for an empty one, and whatever the library says
    /// about the drawing otherwise.
    pub fn open_path(decoder: &Decoder, path: impl AsRef<Path>, limits: &Limits) -> Result<Self> {
        let path = path.as_ref();
        let text = path.to_str().ok_or(Error::PathNotUtf8)?;
        if text.is_empty() {
            return Err(Error::EmptyInput);
        }

        let dwg = decoder.capabilities.dwg();
        let handle = sys::open_path(text.as_bytes(), limits, dwg)?;
        Ok(Self::wrap(decoder, handle))
    }

    /// Opens a drawing from bytes the caller owns.
    ///
    /// The library reads the buffer during the call and never keeps it, so it
    /// may be freed the moment this returns. Use it for a drawing that is
    /// already in memory; for one on disk, [`Document::open_path`] saves the
    /// copy.
    ///
    /// # Errors
    ///
    /// [`Error::EmptyInput`] for an empty slice, and whatever the library says
    /// about the bytes otherwise.
    pub fn open_bytes(decoder: &Decoder, bytes: &[u8], limits: &Limits) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::EmptyInput);
        }

        let dwg = decoder.capabilities.dwg();
        let handle = sys::open_memory(bytes, limits, dwg)?;
        Ok(Self::wrap(decoder, handle))
    }

    fn wrap(decoder: &Decoder, handle: sys::DocumentHandle) -> Self {
        Self {
            handle,
            capabilities: decoder.capabilities.clone(),
            initial_batch_bytes: decoder.initial_batch_bytes,
            max_batch_bytes: decoder.max_batch_bytes,
        }
    }

    /// What the library that opened this document can do.
    #[must_use]
    pub const fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// How many views the document holds. Indices run from zero to one less
    /// than this.
    ///
    /// # Errors
    ///
    /// Whatever the library says.
    pub fn view_count(&self) -> Result<u32> {
        sys::view_count(&self.handle, self.capabilities.dwg())
    }

    /// One view.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an index outside the range, and whatever
    /// the library says otherwise.
    pub fn view(&self, index: u32) -> Result<View> {
        Ok(View::from_raw(sys::view_info(
            &self.handle,
            index,
            self.capabilities.dwg(),
        )?))
    }

    /// Every view, in index order.
    ///
    /// # Errors
    ///
    /// Whatever the library says about the count or about any one view.
    pub fn views(&self) -> Result<Vec<View>> {
        let count = self.view_count()?;
        (0..count).map(|index| self.view(index)).collect()
    }

    /// Begins decoding one view.
    ///
    /// The stream borrows this document, so the document cannot be dropped
    /// while the stream is alive.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an index outside the range, and whatever
    /// the library says otherwise.
    pub fn decode(&self, view_index: u32) -> Result<PrimitiveStream<'_>> {
        self.begin(view_index, None)
    }

    /// Begins decoding one view under a cancel flag.
    ///
    /// The decoder reads the flag between batches, so a cancel arrives at the
    /// next batch boundary and never in the middle of a native parse. It is
    /// final for that decode: clearing the flag and asking again gets
    /// [`Error::Cancelled`], not the rest of the drawing.
    ///
    /// # Errors
    ///
    /// As [`Document::decode`]. Beginning a decode under a flag that is
    /// already set is legal, and the first item is the cancellation.
    pub fn decode_with_cancel(
        &self,
        view_index: u32,
        cancel: &CancelToken,
    ) -> Result<PrimitiveStream<'_>> {
        self.begin(view_index, Some(cancel))
    }

    fn begin(&self, view_index: u32, cancel: Option<&CancelToken>) -> Result<PrimitiveStream<'_>> {
        let handle = sys::decode_begin(
            &self.handle,
            view_index,
            cancel.map(CancelToken::shared),
            self.capabilities.dwg(),
        )?;
        Ok(PrimitiveStream::new(
            self,
            handle,
            self.capabilities.dwg(),
            self.initial_batch_bytes,
            self.max_batch_bytes,
        ))
    }
}

/// What a view is: model space, or one of the paper-space layouts.
///
/// An enum with an `Unknown` arm rather than a closed pair, because the
/// boundary's `kind` is a `uint32_t` and 2 already means unknown. A closed two
/// variant enum could not represent the value the contract defines today, let
/// alone one it adds later. [`View::raw_kind`] is the number itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ViewKind {
    /// Model space.
    Model,
    /// One of the paper-space layouts.
    Layout,
    /// The source did not say, or said something this build has no name for.
    Unknown,
}

impl ViewKind {
    /// Reads the boundary's number.
    #[must_use]
    pub const fn from_raw(kind: u32) -> Self {
        match kind {
            0 => Self::Model,
            1 => Self::Layout,
            _ => Self::Unknown,
        }
    }
}

/// A bounding box in drawing units.
///
/// Only ever handed out when it is usable: see [`View::extents`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extents {
    /// The smaller x.
    pub min_x: f64,
    /// The smaller y.
    pub min_y: f64,
    /// The larger x.
    pub max_x: f64,
    /// The larger y.
    pub max_y: f64,
}

impl From<batch::Bounds> for Extents {
    fn from(bounds: batch::Bounds) -> Self {
        Self {
            min_x: bounds.min_x,
            min_y: bounds.min_y,
            max_x: bounds.max_x,
            max_y: bounds.max_y,
        }
    }
}

/// One view of a document.
///
/// The same type comes back from [`Document::views`] and from
/// [`crate::Item::ViewBegin`], because the struct the boundary fills and the
/// record the wire carries describe the same thing with the same fields.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    index: u32,
    kind: u32,
    extents: [f64; 4],
    entity_count: u64,
    name: String,
}

impl View {
    pub(crate) fn from_raw(raw: sys::RawView) -> Self {
        Self {
            index: raw.index,
            kind: raw.kind,
            extents: raw.extents,
            entity_count: raw.entity_count,
            name: raw.name,
        }
    }

    pub(crate) fn from_record(record: &batch::ViewBegin<'_>) -> Self {
        Self {
            index: record.view_index,
            kind: record.kind,
            extents: record.extents,
            entity_count: record.item_count,
            name: record.name.to_owned(),
        }
    }

    /// This view's index, which is what [`Document::decode`] takes.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The view's name, as UTF-8.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Model space or a layout.
    #[must_use]
    pub const fn kind(&self) -> ViewKind {
        ViewKind::from_raw(self.kind)
    }

    /// The boundary's own number for the kind, for a build that grows one this
    /// crate has no name for.
    #[must_use]
    pub const fn raw_kind(&self) -> u32 {
        self.kind
    }

    /// The view's bounding box, when it has a usable one.
    ///
    /// `None` covers two cases a caller must not tell apart by guessing. A
    /// view whose extents the drawing cannot give reports the inverted box,
    /// `1e20` against `-1e20`, which is the pair AutoCAD writes into its own
    /// `EXTMIN` and `EXTMAX` for an empty drawing, and `1e20` is not to be
    /// read as an extent. And a box carrying a `NaN` or an infinity is not a
    /// box: `NaN > NaN` is false, so a comparison-only check would hand one
    /// back and call it usable, and one `NaN` poisons every union downstream.
    ///
    /// Read this beside the warnings in the same view.
    /// [`crate::WarningCode::EMPTY_VIEW`] alone is a view with nothing in it;
    /// the same warning with others around it is a view something went wrong
    /// reading. [`View::raw_extents`] is the four numbers either way.
    #[must_use]
    pub fn extents(&self) -> Option<Extents> {
        let [min_x, min_y, max_x, max_y] = self.extents;
        if !self.extents.iter().all(|v| v.is_finite()) {
            return None;
        }
        if min_x > max_x || min_y > max_y {
            return None;
        }
        Some(Extents {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// The four numbers the boundary gave, in the order it declares them, with
    /// no rule applied.
    #[must_use]
    pub const fn raw_extents(&self) -> [f64; 4] {
        self.extents
    }

    /// Entities before any expansion of nested insertions, for progress
    /// reporting only.
    ///
    /// It is an upper bound on nothing and a lower bound on nothing, so it
    /// must never size a buffer or decide that a decode finished. The
    /// `DocumentEnd` totals are what say a decode finished; see
    /// [`crate::PrimitiveStream::is_complete`].
    #[must_use]
    pub const fn entity_count(&self) -> u64 {
        self.entity_count
    }
}
