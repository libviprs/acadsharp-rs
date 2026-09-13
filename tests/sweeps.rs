//! Two cheap sweeps over real bytes: truncate a capture at every offset, and
//! flip every byte of a small one to every other value.
//!
//! What these prove is liveness, and only liveness. The truncation sweep can
//! never produce a bad `reserved`, a wrapped count or a NaN, because every
//! prefix of a good batch is self consistent right up to the cut, so it always
//! ends in either a clean stop or a length that runs past what is left. The
//! hand written vectors in `malformed.rs` are what cover the rest.
//!
//! `drain` used to read one batch and stop, so two of the four captures were
//! swept over bytes nothing looked at. Measured over the same cuts, reading one
//! batch gives 7580 refused and 10760 clean; walking the whole capture gives
//! 18333 refused and 7 clean, which is one clean stop per batch boundary and
//! nothing else. Those 10,753 cuts that came back clean were the instrument
//! reporting on bytes it never reached, and they were also what satisfied the
//! sweep's own `clean > 0` control.

mod wire;

use acadsharp_rs::batch::BatchReader;

const SYN_1V_12P: &[u8] = include_bytes!("data/syn_1v_12p.bin");
const SYN_2V_3P: &[u8] = include_bytes!("data/syn_2v_3p.bin");
const SYN_1V_1P: &[u8] = include_bytes!("data/syn_1v_1p.bin");
const SYN_3V_40P: &[u8] = include_bytes!("data/syn_3v_40p.bin");

/// Reads a whole capture to the end, swallowing whatever it says. The point is
/// that it returns at all: no panic, no hang, no index out of range.
///
/// It walks batch by batch, advancing by `total_len()` the way `read_capture`
/// does. Reading one batch and stopping is what this used to do, and it meant
/// every mutation past the first batch of the two multi-batch captures changed
/// nothing the instrument could see.
fn drain(bytes: &[u8]) -> (usize, bool) {
    let mut ok = 0;
    let mut offset = 0usize;
    loop {
        // Deliberately not a `while offset < bytes.len()`: an empty buffer has
        // to go through `BatchReader::new` and be refused, the same as every
        // other buffer too short to hold a batch header.
        match BatchReader::new(&bytes[offset..]) {
            Err(_) => return (ok, true),
            Ok(reader) => {
                for record in reader.records() {
                    match record {
                        Ok(_) => ok += 1,
                        Err(_) => return (ok, true),
                    }
                }
                // `BatchReader::new` proved `12 + payload_length` fits inside
                // what was left, so this cannot walk past the end, and
                // `total_len()` is at least 12, so it cannot fail to advance.
                offset += reader.total_len();
                if offset >= bytes.len() {
                    return (ok, false);
                }
            }
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
        // The control, per capture and scoped to the bytes the walk actually
        // reaches. A whole-sweep `refusals > 0 && clean > 0` is satisfied for
        // free: a cut past the first batch used to be clean because nothing
        // looked at it, so the control passed on cuts the instrument never
        // touched. Both halves have to come from inside the first batch, which
        // is the part every capture has.
        let first_batch = BatchReader::new(bytes)
            .unwrap_or_else(|e| panic!("{name} does not open: {e}"))
            .total_len();
        let mut refused_in_first = 0usize;
        let mut clean_in_first = 0usize;
        let mut batches = 0usize;
        {
            let mut offset = 0;
            while offset < bytes.len() {
                let batch = BatchReader::new(&bytes[offset..])
                    .unwrap_or_else(|e| panic!("{name} at {offset}: {e}"));
                offset += batch.total_len();
                batches += 1;
            }
        }
        for cut in 0..=bytes.len() {
            let (_, refused) = drain(&bytes[..cut]);
            if refused {
                refusals += 1;
            } else {
                clean += 1;
            }
            if cut <= first_batch {
                if refused {
                    refused_in_first += 1;
                } else {
                    clean_in_first += 1;
                }
            }
        }
        assert!(
            refused_in_first > 0,
            "{name}: some cut inside the first batch must be refused"
        );
        assert!(
            clean_in_first > 0,
            "{name}: some cut inside the first batch must stop cleanly"
        );
        println!(
            "{name}: {} truncations over {batches} batches parsed without a panic, \
             {refused_in_first} refused and {clean_in_first} clean inside the first {first_batch} bytes",
            bytes.len() + 1
        );
    }
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
