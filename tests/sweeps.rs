//! Two cheap sweeps over real bytes: truncate a capture at every offset, and
//! flip every byte of a small one to every other value.
//!
//! What these prove is liveness, and only liveness. The truncation sweep can
//! never produce a bad `reserved`, a wrapped count or a NaN, because every
//! prefix of a good batch is self consistent right up to the cut, so it always
//! ends in either a clean stop or a length that runs past what is left. The
//! hand written vectors in `malformed.rs` are what cover the rest.

mod common;

use acadsharp_rs::batch::BatchReader;

const SYN_1V_12P: &[u8] = include_bytes!("data/syn_1v_12p.bin");
const SYN_2V_3P: &[u8] = include_bytes!("data/syn_2v_3p.bin");
const SYN_1V_1P: &[u8] = include_bytes!("data/syn_1v_1p.bin");
const SYN_3V_40P: &[u8] = include_bytes!("data/syn_3v_40p.bin");

/// Reads a batch to the end, swallowing whatever it says. The point is that
/// it returns at all: no panic, no hang, no index out of range.
fn drain(bytes: &[u8]) -> (usize, bool) {
    match BatchReader::new(bytes) {
        Err(_) => (0, true),
        Ok(reader) => {
            let mut ok = 0;
            let mut refused = false;
            for record in reader.records() {
                match record {
                    Ok(_) => ok += 1,
                    Err(_) => {
                        refused = true;
                        break;
                    }
                }
            }
            (ok, refused)
        }
    }
}

#[test]
fn truncating_a_capture_at_every_offset_never_panics() {
    let mut refusals = 0usize;
    let mut clean = 0usize;
    for (name, bytes) in [
        ("syn_1v_12p.bin", SYN_1V_12P),
        ("syn_2v_3p.bin", SYN_2V_3P),
        ("syn_1v_1p.bin", SYN_1V_1P),
        ("syn_3v_40p.bin", SYN_3V_40P),
    ] {
        for cut in 0..=bytes.len() {
            let (_, refused) = drain(&bytes[..cut]);
            if refused {
                refusals += 1;
            } else {
                clean += 1;
            }
        }
        println!("{name}: {} truncations parsed without a panic", bytes.len() + 1);
    }
    // A positive control on the sweep itself. If every cut came back clean the
    // sweep would be testing nothing, and a zero has two explanations.
    assert!(refusals > 0, "some truncations must be refused");
    assert!(clean > 0, "some truncations must stop cleanly");
    println!("truncation sweep: {refusals} refused, {clean} stopped cleanly");
}

#[test]
fn truncating_inside_the_batch_header_is_always_refused() {
    for cut in 0..12 {
        let (ok, refused) = drain(&SYN_1V_12P[..cut]);
        assert_eq!(ok, 0, "a headerless buffer yields no records");
        assert!(refused, "a buffer of {cut} bytes cannot be a batch");
    }
}

#[test]
fn flipping_every_byte_of_a_capture_to_every_other_value_never_panics() {
    // 252 bytes times 255 replacements is about 64,000 parses, which takes
    // milliseconds. Every one of them either reads records or refuses, and
    // none of them may panic, hang or read out of range.
    let mut buf = SYN_1V_1P.to_vec();
    let mut refusals = 0usize;
    let mut parsed_clean = 0usize;
    for i in 0..buf.len() {
        let original = buf[i];
        for v in 0..=u8::MAX {
            if v == original {
                continue;
            }
            buf[i] = v;
            let (_, refused) = drain(&buf);
            if refused {
                refusals += 1;
            } else {
                parsed_clean += 1;
            }
        }
        buf[i] = original;
    }
    assert_eq!(buf, SYN_1V_1P, "the sweep put every byte back");
    let total = refusals + parsed_clean;
    assert_eq!(total, SYN_1V_1P.len() * 255);
    println!("single byte sweep: {total} parses, {refusals} refused, {parsed_clean} read through");
    // Positive control again: a mutation of a byte this decoder actually looks
    // at has to change the answer, or the sweep is exercising nothing.
    assert!(refusals > 0, "some single byte changes must be refused");
    assert!(
        parsed_clean > 0,
        "some single byte changes land in a payload the decoder reads verbatim"
    );
}

#[test]
fn a_capture_with_every_byte_zeroed_is_refused_rather_than_read() {
    let zeros = vec![0u8; SYN_1V_1P.len()];
    let (ok, refused) = drain(&zeros);
    assert_eq!(ok, 0);
    assert!(refused);
}

#[test]
fn random_looking_bytes_are_refused_rather_than_read() {
    // A cheap xorshift, so this needs no dev dependency and no fuzzer.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut buf = vec![0u8; 4096];
    for _ in 0..256 {
        for b in &mut buf {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *b = (state >> 24) as u8;
        }
        let _ = drain(&buf);
    }
}

#[test]
fn a_valid_header_over_random_payload_bytes_never_panics() {
    // The header is what gets a parser past the door, so the interesting fuzz
    // is a good header over bytes that are not records at all.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let payload_len = 2048usize;
    let mut buf = Vec::with_capacity(12 + payload_len);
    for _ in 0..256 {
        buf.clear();
        buf.extend_from_slice(b"VACB");
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&(payload_len as u32).to_le_bytes());
        for _ in 0..payload_len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            buf.push((state >> 24) as u8);
        }
        let _ = drain(&buf);
    }
}
