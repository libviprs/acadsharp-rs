//! The batch buffer: how it grows, how far, and why `BUFFER_TOO_SMALL` never
//! reaches a caller.
//!
//! That code is about memory this crate owns and nothing about the decode, so
//! surfacing it would be handing a caller a problem they cannot act on. The
//! rule is: grow to exactly the size the callee reported, retry at most once,
//! and cap the growth at a crate-side ceiling so one legal record near
//! 2^31 - 1 bytes is a typed refusal rather than a two gigabyte allocation.
//!
//! The numbers below are measured against the pinned archive. They are worth
//! pinning because the library packs a batch to fit the capacity it was
//! handed, so the sizes it asks for are a property of this stream and this
//! starting buffer rather than a constant: 36, then 84, then 204, then 252.
//! Not one of them is a doubling of 12, which is the whole point.
#![cfg(acadsharp_linked)]

use acadsharp_rs::{Decoder, Document, Error, Item, Limits};

fn synthetic() -> Vec<u8> {
    let mut bytes = b"VIPRSSYN".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes
}

#[test]
fn the_defaults_are_the_documented_batch_target_and_a_sane_ceiling() {
    let decoder = Decoder::new().expect("the handshake passes");
    assert_eq!(
        decoder.initial_batch_bytes(),
        64 * 1024,
        "batches target 64 KiB, so the common case never round trips"
    );
    assert_eq!(decoder.max_batch_bytes(), 64 * 1024 * 1024);
}

#[test]
fn the_buffer_grows_to_exactly_what_the_library_asked_for_and_never_by_doubling() {
    let decoder = Decoder::new()
        .expect("the handshake passes")
        .with_initial_batch_bytes(12);
    let document = Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    let mut sizes = Vec::new();
    let mut items = 0usize;
    while let Some(item) = stream.next() {
        item.expect("the grow and retry is invisible to the caller");
        items += 1;
        sizes.push(stream.buffer_len());
    }
    sizes.dedup();

    assert_eq!(items, 17, "the whole stream still arrives");
    assert!(stream.is_complete());
    assert_eq!(
        sizes,
        vec![36, 84, 204, 252],
        "every size is one the library named, and a doubling from 12 reaches 12, 24, 48, \
         96, 192, 384 and none of these"
    );
    assert_eq!(stream.buffer_len(), 252);
    assert!(
        stream.batches_pulled() > 1,
        "a small buffer means the library packs more, smaller batches"
    );
}

#[test]
fn an_initial_size_below_the_batch_header_is_raised_to_it() {
    // A cap in 1..=11 comes back as BUFFER_TOO_SMALL with 12 in `written`,
    // which is the smallest legal batch rather than the size of this one. A
    // caller who started there would spend its one retry learning that, so
    // the floor is 12 and the retry is spent on the real answer.
    let decoder = Decoder::new()
        .expect("the handshake passes")
        .with_initial_batch_bytes(1);
    assert_eq!(decoder.initial_batch_bytes(), 12);

    let document = Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");
    let count = (&mut stream).filter(|item| item.is_ok()).count();
    assert_eq!(count, 17);
    assert!(stream.is_complete());
}

#[test]
fn a_batch_past_the_crate_side_ceiling_is_a_typed_refusal_and_not_an_allocation() {
    let decoder = Decoder::new()
        .expect("the handshake passes")
        .with_initial_batch_bytes(12)
        .with_max_batch_bytes(64);
    let document = Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    // The first batch fits under the ceiling, so the refusal is not just "this
    // is a silly ceiling": the stream really is walking and then stops.
    assert_eq!(
        stream.next().map(|item| item.map(|i| i.record_type())),
        Some(Ok(1))
    );
    // Matched rather than built: `BatchTooLarge` is `#[non_exhaustive]`, so a
    // consumer reads it and never constructs one. Both numbers are still
    // asserted, because a caller can only act on this by raising one of them.
    let refusal = stream
        .next()
        .expect("a second item")
        .expect_err("it is a refusal");
    let Error::BatchTooLarge {
        required,
        max_batch_bytes,
        ..
    } = refusal
    else {
        panic!("a batch past the ceiling is BatchTooLarge, got {refusal:?}");
    };
    assert_eq!((required, max_batch_bytes), (84, 64));
    assert_eq!(
        stream.next(),
        None,
        "and it is a refusal, so the stream is over"
    );
    assert!(
        stream.buffer_len() <= 64,
        "nothing was allocated to hold the batch that did not fit, and the buffer is {}",
        stream.buffer_len()
    );
    assert!(!stream.is_complete());
}

#[test]
fn nothing_in_a_normal_walk_ever_sees_the_buffer_code() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document = Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");
    let items: Vec<Item> = (&mut stream)
        .map(|item| item.expect("a default sized buffer never round trips"))
        .collect();

    assert_eq!(items.len(), 17);
    assert_eq!(
        stream.buffer_len(),
        64 * 1024,
        "and the buffer never grew, because the batch fitted"
    );
    assert_eq!(stream.batches_pulled(), 1);
}
