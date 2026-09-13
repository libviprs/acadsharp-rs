//! The decode stream: one native batch at a time, one item at a time.
//!
//! # What this has to get right
//!
//! **The latch.** Once a decode fails for a reason of its own, every later
//! call on the same handle returns that same code and reports `done` 0. That
//! covers a cancellation, a bound, and anything the library reports as a bug
//! of its own. So the stream yields the error once and then [`None`] forever,
//! and it implements [`FusedIterator`] to say so. An iterator that called
//! again after yielding an error would spin forever hammering a library that
//! has already made up its mind. It also stops at `done == 1` rather than
//! calling again, which is legal but pointless.
//!
//! That rule is here because its absence was a silent truncation: without the
//! latch, the call after a breached bound framed an empty batch, set the last
//! batch flag, reported done and returned success, so a caller who grew its
//! buffer and retried got a well formed complete-looking stream with every
//! record after the breach missing and no code to look at.
//!
//! **The buffer.** `VIPRS_ACAD_BUFFER_TOO_SMALL` is about memory this crate
//! owns and says nothing about the decode, so it never reaches a caller. The
//! buffer grows to exactly the size the callee reported, never by doubling,
//! because the callee reported the exact size of a batch that will not change.
//! It retries at most once: a second refusal for the same batch is a contract
//! violation and becomes [`Error::Internal`], never a loop. And the growth is
//! capped, because a single legal record can approach 2^31 - 1 bytes.
//!
//! **Progress.** Every turn of the loop either yields a record or pulls a
//! batch, and a pull that comes back with nothing in it has moved nothing. A
//! source that answers `OK` with a well formed twelve byte batch and `done` 0
//! for ever is not failing, so the latch never sees it, and the loop would
//! spin on it at about ten million calls a second. So a run of payload-free
//! non-final batches is counted and [`MAX_EMPTY_BATCHES_IN_A_ROW`] of them in
//! a row is [`Error::Internal`]. [`crate::batch`] carries a hard iteration
//! bound one layer down for the same reason.
//!
//! **The totals.** [`PrimitiveStream::is_complete`] is the only proof a caller
//! gets that a decode was not truncated, and it is why the frame records are
//! items rather than something the stream swallows.

use std::iter::FusedIterator;

use crate::batch::{BATCH_HEADER_LEN, BatchReader};
use crate::error::{Error, Result};
use crate::item::Item;
use crate::sys::{self, Dwg};
use crate::{Document, batch};

/// What a new stream's buffer starts at: the documented batch target, so the
/// common case never round trips.
pub(crate) const DEFAULT_INITIAL_BATCH_BYTES: usize = 64 * 1024;

/// How large a single batch this crate will hold by default.
pub(crate) const DEFAULT_MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

/// How many payload-free non-final batches in a row this crate will take
/// before it calls the stream broken.
///
/// A batch of twelve bytes is the frame and nothing else, so it moves the
/// cursor by nothing and the stream is exactly where it was. One of those is a
/// producer framing a boundary and is legal; a run of them is a library that
/// will never finish, and the only thing a caller can do about it is stop.
///
/// Three rather than one, so a producer with a reason to pad has room, and the
/// count resets on the first batch that carries a payload, so padding every
/// other batch runs for ever.
pub(crate) const MAX_EMPTY_BATCHES_IN_A_ROW: u32 = 3;

/// One call to `viprs_acad_decode_next_batch`, as it came back.
///
/// `written` and `done` are zeroed before the call and are only meaningful on
/// `OK` and `BUFFER_TOO_SMALL`. That is a rule for the caller rather than a
/// promise from the callee: the pinned library does write both, as zero, on
/// some other codes, and a wrapper that turned that into a guarantee would be
/// inventing one the contract does not make.
pub(crate) struct NativeBatch {
    pub(crate) code: u32,
    pub(crate) written: u64,
    pub(crate) done: u8,
}

/// Where batches come from.
///
/// A trait with exactly one real implementation, the decode handle. It exists
/// so the state machine below can be driven by a scripted sequence of answers
/// in a unit test: the retry bound in particular cannot be reached through a
/// conformant library, because a conformant library never asks twice for one
/// batch, and an untested bound is a bound nobody knows is wired up.
pub(crate) trait BatchSource {
    fn next_batch(&mut self, buf: &mut [u8]) -> NativeBatch;
}

/// The decode stream for one view.
///
/// It borrows the [`Document`] it came from, because `viprs_acad_close`
/// releases every decode handle still open on the document and the ordering is
/// therefore not a matter of taste. Borrowck enforces it.
///
/// ```no_run
/// # fn main() -> Result<(), acadsharp_rs::Error> {
/// use acadsharp_rs::{Decoder, Document, Item, Limits, Primitive};
///
/// let decoder = Decoder::new()?;
/// let document = Document::open_path(&decoder, "plan.dwg", &Limits::new())?;
/// let mut stream = document.decode(0)?;
///
/// for item in &mut stream {
///     if let Item::Primitive(Primitive::Line(line)) = item? {
///         println!("{:?} to {:?}", line.start, line.end);
///     }
/// }
///
/// assert!(stream.is_complete(), "the decode stopped early");
/// # Ok(())
/// # }
/// ```
pub struct PrimitiveStream<'doc> {
    handle: sys::DecodeHandle,
    core: Core,
    /// The borrow that keeps the document alive.
    ///
    /// A real reference rather than a `PhantomData<&'doc Document>`, which
    /// would say the same thing to borrowck and point the compile-fail cases
    /// at core's `marker.rs` for their expected output.
    _document: &'doc Document,
}

impl<'doc> PrimitiveStream<'doc> {
    pub(crate) fn new(
        document: &'doc Document,
        handle: sys::DecodeHandle,
        dwg: Dwg,
        initial_batch_bytes: usize,
        max_batch_bytes: usize,
    ) -> Self {
        Self {
            handle,
            core: Core::new(dwg, initial_batch_bytes, max_batch_bytes),
            _document: document,
        }
    }

    /// The `DocumentBegin` this stream opened with, once it has gone past.
    #[must_use]
    pub const fn document_begin(&self) -> Option<batch::DocumentBegin> {
        self.core.document_begin
    }

    /// The `ViewEnd` that closed this stream's view, once it has gone past,
    /// carrying how many records the view held.
    #[must_use]
    pub const fn view_end(&self) -> Option<batch::ViewEnd> {
        self.core.view_end
    }

    /// The `DocumentEnd` that closed the stream, once it has gone past.
    ///
    /// `None` after a refusal, which is the whole point: a stream that stopped
    /// early has no totals, and that is how a caller tells it apart from one
    /// that finished.
    #[must_use]
    pub const fn document_end(&self) -> Option<batch::DocumentEnd> {
        self.core.document_end
    }

    /// How many records have come out of this stream, frame records included.
    #[must_use]
    pub const fn records_seen(&self) -> u64 {
        self.core.records_seen
    }

    /// How many of those were warnings.
    #[must_use]
    pub const fn warnings_seen(&self) -> u64 {
        self.core.warnings_seen
    }

    /// How many batches have been pulled from the library.
    #[must_use]
    pub const fn batches_pulled(&self) -> u64 {
        self.core.batches_pulled
    }

    /// Whether this stream ran to a `DocumentEnd` whose total matches what
    /// actually came out of it.
    ///
    /// This is the only completeness proof there is. A decode that stopped
    /// early yields its error and then [`None`], and [`None`] on its own looks
    /// exactly like an ending. `total_records` counts every record in the
    /// stream, `DocumentBegin` and `DocumentEnd` included, so the comparison
    /// is direct.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        match self.core.document_end {
            Some(end) => !self.core.failed && end.total_records == self.core.records_seen,
            None => false,
        }
    }

    /// How large the batch buffer currently is, in bytes.
    ///
    /// Worth exposing because it is the one thing a caller cannot see and may
    /// want to bound: it starts at
    /// [`crate::Decoder::with_initial_batch_bytes`] and only ever grows to
    /// exactly what the library asked for.
    #[must_use]
    pub const fn buffer_len(&self) -> usize {
        self.core.buffer_len()
    }

    /// The ceiling this stream inherited from its decoder.
    #[must_use]
    pub const fn max_batch_bytes(&self) -> usize {
        self.core.max_batch_bytes
    }
}

impl core::fmt::Debug for PrimitiveStream<'_> {
    /// Everything but the handle and the buffer: one is an opaque number and
    /// the other is up to 64 MiB of drawing.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrimitiveStream")
            .field("records_seen", &self.core.records_seen)
            .field("warnings_seen", &self.core.warnings_seen)
            .field("batches_pulled", &self.core.batches_pulled)
            .field("buffer_len", &self.core.buffer_len())
            .field("complete", &self.is_complete())
            .finish_non_exhaustive()
    }
}

impl Iterator for PrimitiveStream<'_> {
    type Item = Result<Item>;

    fn next(&mut self) -> Option<Self::Item> {
        self.core.next(&mut self.handle)
    }
}

impl FusedIterator for PrimitiveStream<'_> {}

/// Everything about the stream that does not need a handle.
struct Core {
    buf: Vec<u8>,
    /// Bytes of the current batch sitting in `buf`.
    batch_len: usize,
    /// Where the next record starts, counted from the start of the batch.
    cursor: u64,
    /// The library said this was the last batch.
    native_done: bool,
    /// Batches in a row that carried no payload and did not say `done`. See
    /// [`MAX_EMPTY_BATCHES_IN_A_ROW`].
    empty_batches_in_a_row: u32,
    /// Nothing more will come out of here.
    finished: bool,
    /// Something was refused, so the totals cannot be trusted.
    failed: bool,
    max_batch_bytes: usize,
    dwg: Dwg,
    records_seen: u64,
    warnings_seen: u64,
    batches_pulled: u64,
    document_begin: Option<batch::DocumentBegin>,
    view_end: Option<batch::ViewEnd>,
    document_end: Option<batch::DocumentEnd>,
}

impl Core {
    fn new(dwg: Dwg, initial_batch_bytes: usize, max_batch_bytes: usize) -> Self {
        let initial = initial_batch_bytes.max(BATCH_HEADER_LEN);
        Self {
            buf: vec![0u8; initial],
            batch_len: 0,
            cursor: 0,
            native_done: false,
            empty_batches_in_a_row: 0,
            finished: false,
            failed: false,
            max_batch_bytes: max_batch_bytes.max(initial),
            dwg,
            records_seen: 0,
            warnings_seen: 0,
            batches_pulled: 0,
            document_begin: None,
            view_end: None,
            document_end: None,
        }
    }

    const fn buffer_len(&self) -> usize {
        self.buf.len()
    }

    /// Yields one item, pulling batches as it needs them.
    fn next(&mut self, source: &mut dyn BatchSource) -> Option<Result<Item>> {
        loop {
            if self.finished {
                return None;
            }

            if self.cursor < self.batch_len as u64 {
                return Some(match self.step() {
                    Ok(item) => Ok(item),
                    Err(error) => {
                        self.stop(true);
                        Err(error)
                    }
                });
            }

            if self.native_done {
                // Calling again after `done` is legal and pointless, and the
                // answer would be an empty batch forever.
                self.stop(false);
                return None;
            }

            if let Err(error) = self.pull(source) {
                self.stop(true);
                return Some(Err(error));
            }
        }
    }

    fn stop(&mut self, failed: bool) {
        self.finished = true;
        self.failed |= failed;
    }

    /// Reads one record out of the batch already in the buffer.
    fn step(&mut self) -> Result<Item> {
        let reader = BatchReader::new(&self.buf[..self.batch_len])?;
        let mut records = reader
            .records_from(self.cursor)
            .map_err(|_| Error::Internal {
                what: "this crate lost track of where it was inside a batch",
            })?;

        let Some(record) = records.next() else {
            return Err(Error::Internal {
                what: "a batch with bytes left in it produced no record",
            });
        };
        let record = record?;
        self.cursor = records.offset();
        self.records_seen += 1;

        let item = Item::from_record(&record);
        match &item {
            Item::DocumentBegin(begin) => self.document_begin = Some(*begin),
            Item::ViewEnd(end) => self.view_end = Some(*end),
            Item::DocumentEnd(end) => self.document_end = Some(*end),
            Item::Warning(_) => self.warnings_seen += 1,
            _ => {}
        }
        Ok(item)
    }

    /// Fetches the next batch, growing the buffer at most once.
    fn pull(&mut self, source: &mut dyn BatchSource) -> Result<()> {
        let mut grown = false;
        loop {
            let cap = self.buf.len();
            let answer = source.next_batch(&mut self.buf);

            // `written` and `done` are only read for the two codes that write
            // them.
            match answer.code {
                crate::ffi::VIPRS_ACAD_OK => {
                    let written = usize::try_from(answer.written).map_err(|_| Error::Internal {
                        what: "the library reported a batch larger than this host can address",
                    })?;
                    if written < BATCH_HEADER_LEN || written > cap {
                        return Err(Error::Internal {
                            what: "the library reported a batch size outside the buffer it was \
                                   handed",
                        });
                    }
                    // A batch of exactly the header is the frame and no
                    // records, so this call moved nothing. That is legal once
                    // and is a stream that never ends if it keeps happening,
                    // and a payload is what says the decode is really walking.
                    if written == BATCH_HEADER_LEN && answer.done == 0 {
                        self.empty_batches_in_a_row += 1;
                        if self.empty_batches_in_a_row > MAX_EMPTY_BATCHES_IN_A_ROW {
                            return Err(Error::Internal {
                                what: "the library answered a run of batches with nothing in \
                                       them and never reported done",
                            });
                        }
                    } else {
                        self.empty_batches_in_a_row = 0;
                    }
                    self.batch_len = written;
                    self.cursor = BATCH_HEADER_LEN as u64;
                    self.native_done = answer.done != 0;
                    self.batches_pulled += 1;
                    return Ok(());
                }
                crate::ffi::VIPRS_ACAD_BUFFER_TOO_SMALL => {
                    if grown {
                        // The callee reported the exact size of a batch that
                        // does not change, so asking twice for one batch is a
                        // contract violation. Looping here is how a decoder
                        // hangs.
                        return Err(Error::Internal {
                            what: "the library asked for a bigger buffer twice for one batch",
                        });
                    }
                    let required = answer.written;
                    if required > self.max_batch_bytes as u64 {
                        return Err(Error::BatchTooLarge {
                            required,
                            max_batch_bytes: self.max_batch_bytes as u64,
                        });
                    }
                    let needed = usize::try_from(required).map_err(|_| Error::Internal {
                        what: "the library asked for a batch larger than this host can address",
                    })?;
                    if needed <= cap {
                        return Err(Error::Internal {
                            what: "the library asked for more room than it was given and named a \
                                   size it already had",
                        });
                    }
                    // Exactly what was asked for, never a doubling.
                    self.buf.resize(needed, 0);
                    grown = true;
                }
                code => {
                    return Err(
                        Error::from_native_code(code, self.dwg.0, self.dwg.1).unwrap_or(
                            Error::Internal {
                                what: "the library reported success through the failure path",
                            },
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch carrying one `DocumentEnd`, written by hand rather than by the
    /// decoder these tests drive. Twelve bytes of frame and a 24 byte record.
    fn one_record_batch(total_records: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"VACB");
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(&13u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(&total_records.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes
    }

    enum Answer {
        Batch { bytes: Vec<u8>, done: bool },
        TooSmall(u64),
        Code(u32),
    }

    struct Scripted {
        answers: Vec<Answer>,
        /// Which answer is next. A batch that did not fit does not advance it,
        /// because nothing was written and nothing was consumed: the batch
        /// that did not fit is still the next one.
        index: usize,
        /// How many times the stream called, which is the number the retry
        /// bound is about.
        calls: usize,
    }

    impl Scripted {
        fn new(answers: Vec<Answer>) -> Self {
            Self {
                answers,
                index: 0,
                calls: 0,
            }
        }
    }

    impl BatchSource for Scripted {
        fn next_batch(&mut self, buf: &mut [u8]) -> NativeBatch {
            let answer = self
                .answers
                .get(self.index)
                .unwrap_or_else(|| panic!("the stream called again after {} answers", self.index));
            self.calls += 1;

            match answer {
                Answer::Batch { bytes, done } => {
                    if bytes.len() > buf.len() {
                        return NativeBatch {
                            code: crate::ffi::VIPRS_ACAD_BUFFER_TOO_SMALL,
                            written: bytes.len() as u64,
                            done: 0,
                        };
                    }
                    self.index += 1;
                    buf[..bytes.len()].copy_from_slice(bytes);
                    NativeBatch {
                        code: crate::ffi::VIPRS_ACAD_OK,
                        written: bytes.len() as u64,
                        done: u8::from(*done),
                    }
                }
                Answer::TooSmall(required) => {
                    self.index += 1;
                    NativeBatch {
                        code: crate::ffi::VIPRS_ACAD_BUFFER_TOO_SMALL,
                        written: *required,
                        done: 0,
                    }
                }
                Answer::Code(code) => {
                    self.index += 1;
                    NativeBatch {
                        code: *code,
                        written: 0,
                        done: 0,
                    }
                }
            }
        }
    }

    fn core(initial: usize, max: usize) -> Core {
        Core::new((1014, 1032), initial, max)
    }

    #[test]
    fn the_buffer_grows_to_exactly_what_was_asked_for_and_retries_once() {
        let batch = one_record_batch(1);
        let mut source = Scripted::new(vec![Answer::Batch {
            bytes: batch.clone(),
            done: true,
        }]);
        let mut core = core(BATCH_HEADER_LEN, usize::MAX);

        let item = core.next(&mut source).expect("an item").expect("it parses");
        assert_eq!(item.record_type(), 13);
        assert_eq!(
            core.buffer_len(),
            batch.len(),
            "grown to the reported size, and a doubling from 12 could not land on {}",
            batch.len()
        );
        assert_eq!(source.calls, 2, "one refusal, one retry");
        assert_eq!(core.next(&mut source), None);
    }

    #[test]
    fn a_second_refusal_for_one_batch_is_a_bug_and_not_a_loop() {
        let mut source = Scripted::new(vec![Answer::TooSmall(64), Answer::TooSmall(4096)]);
        let mut core = core(BATCH_HEADER_LEN, usize::MAX);

        let error = core
            .next(&mut source)
            .expect("a refusal")
            .expect_err("the second ask is a contract violation");
        assert!(matches!(error, Error::Internal { .. }), "got {error:?}");
        assert_eq!(source.calls, 2, "it asked twice and then stopped");
        assert_eq!(core.next(&mut source), None);
        assert_eq!(source.calls, 2, "and it never called again");
    }

    #[test]
    fn a_batch_past_the_ceiling_is_refused_without_growing_the_buffer() {
        let mut source = Scripted::new(vec![Answer::TooSmall(1 << 30)]);
        let mut core = core(1024, 4096);

        assert_eq!(
            core.next(&mut source),
            Some(Err(Error::BatchTooLarge {
                required: 1 << 30,
                max_batch_bytes: 4096,
            }))
        );
        assert_eq!(core.buffer_len(), 1024, "nothing was allocated to hold it");
        assert_eq!(source.calls, 1);
    }

    #[test]
    fn a_latched_code_is_yielded_once_and_then_never_asked_for_again() {
        let mut source = Scripted::new(vec![
            Answer::Batch {
                bytes: one_record_batch(1),
                done: false,
            },
            Answer::Code(crate::ffi::VIPRS_ACAD_CANCELED),
        ]);
        let mut core = core(4096, usize::MAX);

        assert!(core.next(&mut source).expect("an item").is_ok());
        assert_eq!(core.next(&mut source), Some(Err(Error::Cancelled)));
        for _ in 0..5 {
            assert_eq!(core.next(&mut source), None);
        }
        assert_eq!(source.calls, 2, "the latch is what stops the hammering");
        assert!(!core_is_complete(&core));
    }

    #[test]
    fn done_stops_the_calls_rather_than_asking_for_an_empty_batch() {
        let mut source = Scripted::new(vec![Answer::Batch {
            bytes: one_record_batch(1),
            done: true,
        }]);
        let mut core = core(4096, usize::MAX);

        assert!(core.next(&mut source).expect("an item").is_ok());
        assert_eq!(core.next(&mut source), None);
        assert_eq!(core.next(&mut source), None);
        assert_eq!(source.calls, 1);
        assert!(
            core_is_complete(&core),
            "one record, and the total says one"
        );
    }

    #[test]
    fn a_written_outside_the_buffer_is_a_contract_violation() {
        struct Liar;
        impl BatchSource for Liar {
            fn next_batch(&mut self, buf: &mut [u8]) -> NativeBatch {
                NativeBatch {
                    code: crate::ffi::VIPRS_ACAD_OK,
                    written: buf.len() as u64 + 1,
                    done: 0,
                }
            }
        }
        let mut core = core(4096, usize::MAX);
        let error = core
            .next(&mut Liar)
            .expect("a refusal")
            .expect_err("a batch cannot be longer than the buffer it went into");
        assert!(matches!(error, Error::Internal { .. }), "got {error:?}");
    }

    #[test]
    fn a_written_below_the_batch_header_is_a_contract_violation() {
        struct Short;
        impl BatchSource for Short {
            fn next_batch(&mut self, _buf: &mut [u8]) -> NativeBatch {
                NativeBatch {
                    code: crate::ffi::VIPRS_ACAD_OK,
                    written: 11,
                    done: 1,
                }
            }
        }
        let mut core = core(4096, usize::MAX);
        let error = core
            .next(&mut Short)
            .expect("a refusal")
            .expect_err("eleven bytes is not a batch");
        assert!(matches!(error, Error::Internal { .. }), "got {error:?}");
    }

    /// A well formed batch with nothing in it: twelve bytes of frame, a
    /// `payload_length` of 0, and the last-batch flag clear.
    fn payload_free_batch() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"VACB");
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn a_run_of_payload_free_batches_is_refused_rather_than_spun_on() {
        // The shape that used to spin. `written` is 12 so the cursor lands on
        // the batch length and nothing steps, `done` is 0 so the stream pulls
        // again, and the pull sets exactly the state it started from. Nothing
        // here is a failure the latch could catch: every answer is OK.
        struct Treadmill {
            calls: usize,
        }

        /// Far above the bound and far below a spin. A stream with no bound
        /// blows this in well under a second.
        const CALL_BUDGET: usize = 64;

        impl BatchSource for Treadmill {
            fn next_batch(&mut self, buf: &mut [u8]) -> NativeBatch {
                self.calls += 1;
                assert!(
                    self.calls <= CALL_BUDGET,
                    "the stream has asked for {} batches without a single byte of payload \
                     coming back, so it is spinning rather than decoding",
                    self.calls
                );
                let bytes = payload_free_batch();
                buf[..bytes.len()].copy_from_slice(&bytes);
                NativeBatch {
                    code: crate::ffi::VIPRS_ACAD_OK,
                    written: bytes.len() as u64,
                    done: 0,
                }
            }
        }

        let mut source = Treadmill { calls: 0 };
        let mut core = core(4096, usize::MAX);

        let error = core
            .next(&mut source)
            .expect("a refusal")
            .expect_err("a library that never makes progress is a broken library");
        assert!(matches!(error, Error::Internal { .. }), "got {error:?}");
        assert!(
            source.calls <= 4,
            "it asked {} times before giving up, and the bound wants to be small enough that a \
             spin is impossible and large enough that a padding batch survives",
            source.calls
        );
        assert_eq!(core.next(&mut source), None, "and the refusal latches");
    }

    #[test]
    fn a_padding_batch_between_two_real_ones_is_not_a_refusal() {
        // The bound has to leave room for the legitimate case: a producer that
        // frames an empty batch and then carries on. One payload is enough to
        // put the count back to zero, so a padding batch every other batch is
        // fine forever.
        let mut source = Scripted::new(vec![
            Answer::Batch {
                bytes: payload_free_batch(),
                done: false,
            },
            Answer::Batch {
                bytes: one_record_batch(2),
                done: false,
            },
            Answer::Batch {
                bytes: payload_free_batch(),
                done: false,
            },
            Answer::Batch {
                bytes: one_record_batch(2),
                done: true,
            },
        ]);
        let mut core = core(4096, usize::MAX);

        let first = core.next(&mut source).expect("an item").expect("it parses");
        assert_eq!(first.record_type(), 13);
        let second = core.next(&mut source).expect("an item").expect("it parses");
        assert_eq!(second.record_type(), 13);
        assert_eq!(core.next(&mut source), None);
        assert!(
            core_is_complete(&core),
            "two records, and the total says two"
        );
    }

    #[test]
    fn a_corrupt_batch_arrives_as_a_batch_error_with_its_offset() {
        let mut bytes = one_record_batch(1);
        bytes[0] = b'X';
        let mut source = Scripted::new(vec![Answer::Batch { bytes, done: true }]);
        let mut core = core(4096, usize::MAX);

        let error = core
            .next(&mut source)
            .expect("a refusal")
            .expect_err("that is not a VACB batch");
        assert!(matches!(error, Error::Batch(_)), "got {error:?}");
    }

    /// `is_complete` lives on the public type, so this is the same comparison
    /// spelled once for the state machine's own tests.
    fn core_is_complete(core: &Core) -> bool {
        match core.document_end {
            Some(end) => !core.failed && end.total_records == core.records_seen,
            None => false,
        }
    }
}
