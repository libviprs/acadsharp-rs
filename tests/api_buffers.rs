//! The batch buffer: how it grows, how far, and why `BUFFER_TOO_SMALL` never
//! reaches a caller.
//!
//! That code is about memory this crate owns and nothing about the decode, so
//! surfacing it would be handing a caller a problem they cannot act on. The
//! rule is: grow to exactly the size the callee reported, retry at most once,
//! and cap the growth at a crate-side ceiling so one legal record near
//! 2^31 - 1 bytes is a typed refusal rather than a two gigabyte allocation.
#![cfg(acadsharp_linked)]

use acadsharp_rs::{Decoder, Document, Error, Item, Limits};

/// The committed capture of exactly this document, written by this library.
/// Its length is the size of the one batch the decode produces, which is what
/// the buffer has to grow to, to the byte.
const SYN_1V_12P: &[u8] = include_bytes!("data/syn_1v_12p.bin");

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
fn a_buffer_too_small_to_start_grows_to_exactly_what_the_library_asked_for() {
    let decoder = Decoder::new()
        .expect("the handshake passes")
        .with_initial_batch_bytes(12);
    let document =
        Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    let items: Vec<Item> = (&mut stream)
        .map(|item| item.expect("the grow and retry is invisible to the caller"))
        .collect();

    assert_eq!(items.len(), 17, "the whole stream still arrives");
    assert!(stream.is_complete());
    assert_eq!(
        stream.buffer_len(),
        SYN_1V_12P.len(),
        "grown to exactly the size the callee reported. A doubling from 12 could not land \
         on {} by accident",
        SYN_1V_12P.len()
    );
    assert_eq!(
        stream.batches_pulled(),
        1,
        "one batch, however many calls it took to fit it"
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

    let document =
        Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
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
    let document =
        Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    assert_eq!(
        stream.next(),
        Some(Err(Error::BatchTooLarge {
            required: SYN_1V_12P.len() as u64,
            max_batch_bytes: 64,
        })),
        "the refusal names both numbers, because a caller can only act on it by raising one"
    );
    assert_eq!(stream.next(), None, "and it is a refusal, so the stream is over");
    assert!(stream.buffer_len() <= 64, "nothing was allocated to hold it");
}

#[test]
fn nothing_in_a_normal_walk_ever_sees_the_buffer_code() {
    let decoder = Decoder::new().expect("the handshake passes");
    let document =
        Document::open_bytes(&decoder, &synthetic(), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");
    for item in &mut stream {
        let item = item.expect("a default sized buffer never round trips");
        let _ = item;
    }
    assert_eq!(
        stream.buffer_len(),
        64 * 1024,
        "and the buffer never grew, because the batch fitted"
    );
}
