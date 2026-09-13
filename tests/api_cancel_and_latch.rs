//! Cancellation, and the latch that makes a refusal final.
//!
//! Both halves are about the same promise: a decode that stopped early says so
//! once, and then stops. An iterator that calls again after an error spins
//! forever hammering a library that has already latched the same code, and a
//! caller who cannot tell a complete decode from a truncated one renders a
//! drawing with pieces missing and no indication anything went wrong.
#![cfg(acadsharp_linked)]

use std::iter::FusedIterator;

use acadsharp_rs::{CancelToken, Decoder, Document, Error, Limits, PrimitiveStream};

/// Big enough that the stream needs several batches, so a cancel has somewhere
/// to land that is not the first call.
fn many() -> Vec<u8> {
    let mut bytes = b"VIPRSSYN".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&5000u32.to_le_bytes());
    bytes
}

fn small() -> Vec<u8> {
    let mut bytes = b"VIPRSSYN".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes
}

const fn assert_fused<T: FusedIterator>() {}

#[test]
fn the_stream_is_a_fused_iterator() {
    assert_fused::<PrimitiveStream<'_>>();
}

#[test]
fn a_token_is_send_and_sync_because_it_is_the_one_word_two_threads_touch() {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CancelToken>();
}

#[test]
fn a_flag_already_set_stops_the_decode_on_its_first_call() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document = Document::open_bytes(&decoder, &small(), &Limits::new()).expect("it opens");

    let token = CancelToken::new();
    token.cancel();
    assert!(token.is_cancelled());

    let mut stream = document
        .decode_with_cancel(0, &token)
        .expect("beginning a decode under a set flag is still legal");

    assert_eq!(
        stream.next(),
        Some(Err(Error::Cancelled)),
        "cancellation is checked before work, not after"
    );
    assert_eq!(stream.next(), None, "and then it is over");
    assert_eq!(stream.next(), None);
    assert!(!stream.is_complete());
    assert_eq!(stream.document_end(), None);
}

#[test]
fn cancelling_between_batches_yields_exactly_one_error_and_then_none_forever() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document = Document::open_bytes(&decoder, &many(), &Limits::new()).expect("it opens");

    let token = CancelToken::new();
    let mut stream = document.decode_with_cancel(0, &token).expect("it decodes");

    // Walk into the stream far enough that the first batch is behind us.
    let mut before = 0usize;
    for _ in 0..20 {
        match stream.next() {
            Some(Ok(_)) => before += 1,
            other => panic!("the first twenty items should all parse, got {other:?}"),
        }
    }
    assert!(stream.batches_pulled() >= 1);

    token.cancel();

    let mut errors = Vec::new();
    let mut after = 0usize;
    // A generous bound: if the latch is broken this loop is what stops the
    // test from running forever, and the assertion below is what names it.
    for _ in 0..100_000 {
        match stream.next() {
            Some(Ok(_)) => after += 1,
            Some(Err(e)) => {
                errors.push(e);
                break;
            }
            None => break,
        }
    }

    assert_eq!(
        errors,
        vec![Error::Cancelled],
        "a cancel between batches arrives as exactly one Cancelled, and {before} items came \
         before it with {after} after"
    );
    assert!(
        stream.batches_pulled() > 1,
        "the cancel should have landed on a later batch, not the first"
    );

    for _ in 0..5 {
        assert_eq!(
            stream.next(),
            None,
            "a latched decode yields None forever rather than calling the library again"
        );
    }
    assert!(!stream.is_complete());
    assert_eq!(stream.document_end(), None);
    assert!(stream.records_seen() > 0);
}

#[test]
fn a_clone_of_the_token_cancels_the_same_decode() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document = Document::open_bytes(&decoder, &many(), &Limits::new()).expect("it opens");

    let token = CancelToken::new();
    let elsewhere = token.clone();
    let mut stream = document.decode_with_cancel(0, &token).expect("it decodes");
    assert!(stream.next().is_some());

    // The flag has to outlive the decode handle, which is why the stream holds
    // its own `Arc` clone for its whole life. Dropping every handle the caller
    // has must not free it.
    drop(token);
    elsewhere.cancel();
    drop(elsewhere);

    let mut errors = 0usize;
    for _ in 0..100_000 {
        match stream.next() {
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                assert_eq!(e, Error::Cancelled);
                errors += 1;
                break;
            }
            None => break,
        }
    }
    assert_eq!(errors, 1, "the decode saw the flag through the stream's own clone");
}

#[test]
fn a_stream_that_finishes_cleanly_is_complete_and_then_none_forever() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document = Document::open_bytes(&decoder, &small(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    let items: Vec<_> = stream.by_ref().collect();
    assert!(items.iter().all(Result::is_ok));
    assert!(stream.is_complete());
    assert_eq!(stream.next(), None);
    assert_eq!(stream.next(), None);
}
