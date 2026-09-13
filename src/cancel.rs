//! The one word two threads touch at once.
//!
//! The boundary has no callback in either direction, deliberately: a callback
//! would put a caller's code on the library's stack and every question about
//! which locks are held at that moment has an answer nobody wants to maintain.
//! So cancellation is a flag the decoder polls between batches, and this is
//! the safe shape of it.
//!
//! Two things about the shape matter. The flag lives behind an [`Arc`] so that
//! taking its address is taking the address of a heap allocation rather than
//! of a local that is about to move, and [`crate::PrimitiveStream`] keeps its
//! own clone for its whole life, because the boundary requires the flag to
//! outlive the decode handle it was given to. Dropping every token a caller
//! holds cannot free it out from under a running decode.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A cancel flag a decode polls between batches.
///
/// Clone it freely: every clone names the same flag, and this is the one type
/// in the crate that is [`Send`] and [`Sync`], because cancelling from another
/// thread is the entire point.
///
/// ```
/// use acadsharp_rs::CancelToken;
///
/// let token = CancelToken::new();
/// let elsewhere = token.clone();
/// assert!(!token.is_cancelled());
///
/// elsewhere.cancel();
/// assert!(token.is_cancelled());
/// ```
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    flag: Arc<AtomicU32>,
}

impl CancelToken {
    /// A token nobody has cancelled yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Asks the decode to stop.
    ///
    /// The decoder reads the flag between batches, so the decode stops at the
    /// next batch boundary and never in the middle of a native parse. It is
    /// final: the next call returns the cancellation, and clearing the flag
    /// afterwards does not get the rest of the drawing.
    ///
    /// The store is a release, which is what the boundary asks a caller to do.
    pub fn cancel(&self) {
        self.flag.store(1, Ordering::Release);
    }

    /// Whether anybody has called [`CancelToken::cancel`] on this flag.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire) != 0
    }

    /// The shared flag, for the stream to hold and to point the library at.
    pub(crate) fn shared(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.flag)
    }
}
