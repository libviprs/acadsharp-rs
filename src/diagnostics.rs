//! Counters the crate's own tests read. Not API, and not a stability promise.
//!
//! There is no stub library to count calls into, and there is not going to be
//! one: the only question worth asking is what the real pinned library saw. So
//! the counting happens on this side of the boundary, in the two `Drop` impls
//! that release a handle, and the tests read the deltas around a scope.
//!
//! A close happens once per document and once per decode, so two relaxed
//! atomic increments cost nothing measurable and are always compiled. A
//! `cfg(test)` counter would not have worked: an integration test compiles the
//! library without `cfg(test)`, so it would have read a counter that was never
//! built.
#![doc(hidden)]

use std::sync::atomic::{AtomicU64, Ordering};

static DOCUMENT_CLOSES: AtomicU64 = AtomicU64::new(0);
static DECODE_CLOSES: AtomicU64 = AtomicU64::new(0);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LAST_DOCUMENT_CLOSE: AtomicU64 = AtomicU64::new(0);
static LAST_DECODE_CLOSE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn note_document_close() {
    DOCUMENT_CLOSES.fetch_add(1, Ordering::Relaxed);
    LAST_DOCUMENT_CLOSE.store(SEQUENCE.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
}

pub(crate) fn note_decode_close() {
    DECODE_CLOSES.fetch_add(1, Ordering::Relaxed);
    LAST_DECODE_CLOSE.store(SEQUENCE.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
}

/// How many document handles this process has closed.
#[must_use]
pub fn document_closes() -> u64 {
    DOCUMENT_CLOSES.load(Ordering::Relaxed)
}

/// How many decode handles this process has closed.
#[must_use]
pub fn decode_closes() -> u64 {
    DECODE_CLOSES.load(Ordering::Relaxed)
}

/// When the last document close happened, on a counter shared with
/// [`last_decode_close`], so an ordering can be asserted rather than assumed.
#[must_use]
pub fn last_document_close() -> u64 {
    LAST_DOCUMENT_CLOSE.load(Ordering::Relaxed)
}

/// When the last decode close happened. See [`last_document_close`].
#[must_use]
pub fn last_decode_close() -> u64 {
    LAST_DECODE_CLOSE.load(Ordering::Relaxed)
}
