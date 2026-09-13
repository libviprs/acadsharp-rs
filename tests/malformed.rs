//! One named test per refusal in the contract, each asserting the exact error
//! variant and the exact offset.
//!
//! A truncation sweep cannot produce any of these, because every prefix of a
//! good batch is self consistent up to the cut. That is why they are written
//! by hand.

mod common;

use acadsharp_rs::batch::{BatchError, BatchReader, Reason, Record};
use common::{Builder, canonical, frame_raw};

/// Parses a batch and hands back the first error, whether it came from the
/// batch header or from a record.
fn first_error(bytes: &[u8]) -> BatchError {
    match BatchReader::new(bytes) {
        Err(e) => e,
        Ok(reader) => reader
            .records()
            .find_map(Result::err)
            .unwrap_or_else(|| panic!("these bytes were supposed to be refused and were not")),
    }
}

fn assert_corrupt(bytes: &[u8], offset: u64, reason: Reason) {
    let err = first_error(bytes);
    assert_eq!(
        err,
        BatchError::CorruptInput { offset, reason },
        "wrong refusal: got {err}"
    );
    assert_eq!(err.offset(), offset);
}

// ---------------------------------------------------------------------------
// The batch header
// ---------------------------------------------------------------------------

#[test]
fn magic_that_is_not_vacb_is_corrupt_input_at_offset_zero() {
    let batch = Builder::new().magic(*b"VACC").record(canonical(3)).build();
    assert_corrupt(&batch, 0, Reason::BadMagic);
}

#[test]
fn fewer_than_twelve_bytes_is_corrupt_input_at_offset_zero() {
    let batch = Builder::new().record(canonical(3)).build();
    for len in 0..12 {
        assert_corrupt(&batch[..len], 0, Reason::ShortBatchHeader);
    }
}

#[test]
fn wire_version_one_is_an_abi_mismatch_not_corrupt_input() {
    // The issue text calls this CorruptInput. It is not: a foreign wire
    // version is the two ends of this boundary disagreeing, and the remedy is
    // to rebuild one of them, not to go looking at the drawing.
    let batch = Builder::new().wire_version(1).record(canonical(3)).build();
    let err = first_error(&batch);
    assert_eq!(
        err,
        BatchError::AbiMismatch {
            offset: 4,
            found: 1,
            expected: 2,
        },
        "wrong refusal: got {err}"
    );
    assert_eq!(err.offset(), 4);
}

#[test]
fn a_wire_version_from_the_future_is_an_abi_mismatch_too() {
    let batch = Builder::new().wire_version(3).record(canonical(3)).build();
    assert_eq!(
        first_error(&batch),
        BatchError::AbiMismatch {
            offset: 4,
            found: 3,
            expected: 2,
        }
    );
}

#[test]
fn payload_length_past_the_buffer_is_corrupt_input_at_offset_eight() {
    let batch = Builder::new()
        .record(canonical(3))
        .declared_payload_length(1_000_000)
        .build();
    assert_corrupt(&batch, 8, Reason::PayloadPastBuffer);
}

#[test]
fn payload_length_one_byte_past_the_buffer_is_still_refused() {
    let good = Builder::new().record(canonical(3)).build();
    let declared = u32::try_from(good.len() - 12 + 1).expect("fits");
    let batch = Builder::new()
        .record(canonical(3))
        .declared_payload_length(declared)
        .build();
    assert_corrupt(&batch, 8, Reason::PayloadPastBuffer);
}

// ---------------------------------------------------------------------------
// The record header
// ---------------------------------------------------------------------------

#[test]
fn fewer_than_eight_bytes_left_for_a_record_header_is_refused() {
    // One good record, then four bytes that cannot be a header.
    let batch = Builder::new()
        .record(canonical(3))
        .raw(&[0xAA, 0xBB, 0xCC, 0xDD])
        .build();
    assert_corrupt(&batch, 12 + 72, Reason::ShortRecordHeader);
}

#[test]
fn a_record_length_below_its_own_header_is_refused() {
    // This is the infinite loop. A length of 4 cannot advance the cursor past
    // the header it sits in.
    let batch = Builder::new()
        .record(frame_raw(3, 0, 4, &[0u8; 16]))
        .build();
    assert_corrupt(&batch, 12, Reason::LengthBelowHeader);
}

#[test]
fn a_record_length_of_zero_is_refused() {
    let batch = Builder::new()
        .record(frame_raw(3, 0, 0, &[0u8; 16]))
        .build();
    assert_corrupt(&batch, 12, Reason::LengthBelowHeader);
}

#[test]
fn a_record_length_that_is_not_a_multiple_of_four_is_refused() {
    let batch = Builder::new()
        .record(frame_raw(3, 0, 9, &[0u8; 16]))
        .build();
    assert_corrupt(&batch, 12, Reason::LengthNotMultipleOfFour);
}

#[test]
fn a_record_length_past_the_payload_is_refused() {
    let batch = Builder::new()
        .record(frame_raw(3, 0, 10_000, &[0u8; 64]))
        .build();
    assert_corrupt(&batch, 12, Reason::LengthPastPayload);
}

#[test]
fn a_record_length_one_byte_past_the_payload_is_refused() {
    let good = canonical(3);
    let batch = Builder::new()
        .record(frame_raw(
            3,
            0,
            u32::try_from(good.len() + 4).expect("fits"),
            &good[8..],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::LengthPastPayload);
}

#[test]
fn a_record_header_reserved_that_is_not_zero_is_refused() {
    let good = canonical(3);
    let batch = Builder::new()
        .record(frame_raw(
            3,
            1,
            u32::try_from(good.len()).expect("fits"),
            &good[8..],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::RecordReservedNotZero);
}

#[test]
fn the_second_record_reports_its_own_offset() {
    let batch = Builder::new()
        .record(canonical(3))
        .record(frame_raw(3, 0, 4, &[0u8; 16]))
        .build();
    assert_corrupt(&batch, 12 + 72, Reason::LengthBelowHeader);
}

// ---------------------------------------------------------------------------
// Fixed size records
// ---------------------------------------------------------------------------

#[test]
fn a_fixed_size_record_of_the_wrong_length_is_refused() {
    // Every one of these has exactly one legal length. A record that is four
    // bytes over or under still passes every framing rule, so the type's own
    // check is the only thing that catches it.
    for (record_type, legal) in [
        (1u16, 24usize),
        (3, 72),
        (5, 96),
        (6, 80),
        (12, 24),
        (13, 24),
    ] {
        for wrong in [legal - 4, legal + 4] {
            let payload = vec![0u8; wrong - 8];
            let batch = Builder::new()
                .record(frame_raw(
                    record_type,
                    0,
                    u32::try_from(wrong).expect("fits"),
                    &payload,
                ))
                .build();
            assert_corrupt(&batch, 12, Reason::WrongFixedLength);
        }
    }
}

#[test]
fn an_ellipse_of_the_wrong_length_is_refused() {
    for wrong in [116usize, 124] {
        let payload = vec![0u8; wrong - 8];
        let batch = Builder::new()
            .record(frame_raw(
                7,
                0,
                u32::try_from(wrong).expect("fits"),
                &payload,
            ))
            .build();
        assert_corrupt(&batch, 12, Reason::WrongFixedLength);
    }
}

// ---------------------------------------------------------------------------
// Counted records
// ---------------------------------------------------------------------------

#[test]
fn a_polyline_bulge_count_that_is_neither_zero_nor_the_point_count_is_refused() {
    let batch = Builder::new()
        .record(common::polyline_raw(
            1,
            2,
            0,
            1,
            0,
            &[0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.5],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::BulgeCount);
}

#[test]
fn a_polyline_length_that_does_not_match_its_counts_is_refused() {
    // Claims three vertices, carries two.
    let batch = Builder::new()
        .record(common::polyline_raw(
            1,
            3,
            0,
            0,
            0,
            &[0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::PolylineLengthMismatch);
}

#[test]
fn a_polyline_too_short_to_hold_its_own_counts_is_refused() {
    let batch = Builder::new()
        .record(frame_raw(4, 0, 32, &[0u8; 24]))
        .build();
    assert_corrupt(&batch, 12, Reason::WrongFixedLength);
}

/// The one that matters. `64 + 24n + 8bc` done in `u32` wraps at
/// `n = 2_863_311_531` and comes out to exactly the 72 this record declares,
/// so a `u32` parser accepts it and then reads 68 GB of vertices out of eight
/// bytes. Done in `u64` it comes out to 68,719,476,808 and is refused.
#[test]
fn a_polyline_point_count_that_wraps_u32_is_refused() {
    let record = common::polyline_u32_wrap();
    assert_eq!(record.len(), 72, "the vector is a 72 byte record");

    // The arithmetic, spelled out, so this test says why it is the value it is.
    let n = u64::from(common::WRAP_POINT_COUNT);
    assert_eq!(64 + 24 * n, 68_719_476_808, "in u64 it is 68 GB");
    let wrapped = 64u32
        .wrapping_add(24u32.wrapping_mul(common::WRAP_POINT_COUNT))
        .wrapping_add(0);
    assert_eq!(wrapped, 72, "in u32 it is exactly the declared length");

    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::PolylineLengthMismatch);
}

#[test]
fn a_polygon_with_the_same_defect_is_refused_the_same_way() {
    let mut record = common::polyline_u32_wrap();
    record[0..2].copy_from_slice(&9u16.to_le_bytes());
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::PolylineLengthMismatch);
}

#[test]
fn a_spline_weight_count_that_is_neither_zero_nor_the_control_count_is_refused() {
    let batch = Builder::new()
        .record(common::spline_raw(
            1,
            3,
            0,
            1,
            2,
            1,
            &[0.0],
            &[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            &[1.0],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::WeightCount);
}

#[test]
fn a_spline_length_that_does_not_match_its_counts_is_refused() {
    // Claims four knots, carries one.
    let batch = Builder::new()
        .record(common::spline_raw(
            1,
            3,
            0,
            4,
            1,
            0,
            &[0.0],
            &[[1.0, 2.0, 3.0]],
            &[],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::SplineLengthMismatch);
}

#[test]
fn a_spline_control_count_that_wraps_u32_is_refused() {
    // 24 * 178_956_971 is 4_294_967_304, which is 8 past 2^32. So in u32 the
    // identity comes out to 48 + 8 + 0 + 0 = 56, which is what this record
    // declares.
    let control_count = 178_956_971u32;
    assert_eq!(24u32.wrapping_mul(control_count), 8);
    let batch = Builder::new()
        .record(common::spline_raw(
            1,
            3,
            0,
            0,
            control_count,
            0,
            &[],
            &[],
            &[1.0],
        ))
        .build();
    assert_corrupt(&batch, 12, Reason::SplineLengthMismatch);
}

#[test]
fn a_spline_too_short_to_hold_its_own_counts_is_refused() {
    let batch = Builder::new()
        .record(frame_raw(8, 0, 40, &[0u8; 32]))
        .build();
    assert_corrupt(&batch, 12, Reason::WrongFixedLength);
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

#[test]
fn a_text_length_that_does_not_match_its_byte_len_is_refused() {
    let mut record = common::text(1, [1.0, 2.0, 3.0], 4.0, 5.0, "hi");
    // byte_len sits at payload offset 56, so record offset 64.
    record[64..68].copy_from_slice(&99u32.to_le_bytes());
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::StringLengthMismatch);
}

#[test]
fn a_view_begin_length_that_does_not_match_its_name_len_is_refused() {
    let mut record = common::view_begin(0, 0, [0.0; 4], 0, "Model");
    // name_len sits at payload offset 48, so record offset 56.
    record[56..60].copy_from_slice(&99u32.to_le_bytes());
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::StringLengthMismatch);
}

#[test]
fn a_warning_length_that_does_not_match_its_message_len_is_refused() {
    let mut record = common::warning(100, 0, "hi");
    // message_len sits at payload offset 16, so record offset 24.
    record[24..28].copy_from_slice(&99u32.to_le_bytes());
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::StringLengthMismatch);
}

#[test]
fn a_string_record_too_short_to_hold_its_own_length_field_is_refused() {
    for (record_type, too_short) in [(2u16, 60u32), (10, 68), (11, 28)] {
        let payload = vec![0u8; too_short as usize - 8];
        let batch = Builder::new()
            .record(frame_raw(record_type, 0, too_short, &payload))
            .build();
        assert_corrupt(&batch, 12, Reason::WrongFixedLength);
    }
}

#[test]
fn invalid_utf8_in_a_text_record_is_refused() {
    let record = common::text_raw(1, [1.0, 2.0, 3.0], 4.0, 5.0, &[0xFF, 0xFE], 0, 0);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::InvalidUtf8);
}

#[test]
fn invalid_utf8_in_a_view_name_is_refused() {
    let record = common::view_begin_raw(0, 0, [0.0; 4], 0, &[0x80, 0x80, 0x80], 0, 0);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::InvalidUtf8);
}

#[test]
fn invalid_utf8_in_a_warning_message_is_refused() {
    let record = common::warning_raw(100, 0, &[0xC3], 0, 0, 0);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::InvalidUtf8);
}

#[test]
fn a_truncated_multibyte_character_is_refused() {
    // The lead byte of A-umlaut with its continuation byte cut off, which is
    // what a producer that cut a string at a byte rather than a character
    // boundary would send.
    let record = common::text_raw(1, [1.0, 2.0, 3.0], 4.0, 5.0, b"ab\xc3", 0, 0);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::InvalidUtf8);
}

#[test]
fn a_warning_code_of_zero_is_refused() {
    let record = common::warning(0, 0, "zero is not a warning code");
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::ZeroWarningCode);
}

// ---------------------------------------------------------------------------
// Non finite floats in geometry
// ---------------------------------------------------------------------------

#[test]
fn a_nan_coordinate_in_a_line_is_refused() {
    let record = common::line(1, [0.0, 1.0, 2.0, f64::NAN, 4.0, 5.0]);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn an_infinite_radius_in_an_arc_is_refused() {
    let record = common::arc(1, [0.0, 0.0, 0.0], f64::INFINITY, 0.0, 1.0, [0.0, 0.0, 1.0]);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn a_negative_infinity_in_a_circle_is_refused() {
    let record = common::circle(1, [0.0, 0.0, f64::NEG_INFINITY], 1.0, [0.0, 0.0, 1.0]);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn a_nan_in_an_ellipse_parameter_is_refused() {
    let record = common::ellipse(
        1,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        0.5,
        [0.0, f64::NAN],
        [0.0, 0.0, 1.0],
    );
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn a_nan_in_a_polyline_normal_is_refused() {
    let record = common::polyline(1, false, [0.0, 0.0, f64::NAN], &[[1.0, 2.0, 3.0]], &[]);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn a_nan_bulge_is_refused() {
    // The bulges sit past every vertex, so this one only fails if the check
    // covers the whole trailing array rather than the fixed prefix.
    let record = common::polyline(
        1,
        false,
        [0.0, 0.0, 1.0],
        &[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
        &[0.0, f64::NAN],
    );
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn an_infinite_spline_weight_is_refused() {
    let record = common::spline(
        1,
        1,
        0,
        &[0.0, 1.0],
        &[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
        &[1.0, f64::INFINITY],
    );
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn a_nan_in_a_text_height_is_refused() {
    let record = common::text(1, [1.0, 2.0, 3.0], f64::NAN, 5.0, "hi");
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::NonFiniteFloat);
}

#[test]
fn the_finiteness_scan_stops_before_a_text_string() {
    // The scan covers the five doubles at payload 16 to 56 and nothing else.
    // These string bytes spell a NaN and an infinity, so a scan that ran to
    // the end of the payload would call this NonFiniteFloat. It is not: those
    // bytes are the drawing's text, and what is actually wrong with them is
    // that no NaN is valid UTF-8 (0xF8 is not a legal lead byte), which is the
    // refusal I want. The finiteness check runs before the UTF-8 check, so the
    // two answers really are distinguishable here.
    let mut bytes = f64::NAN.to_le_bytes().to_vec();
    bytes.extend_from_slice(&f64::INFINITY.to_le_bytes());
    let record = common::text_raw(1, [1.0, 2.0, 3.0], 4.0, 5.0, &bytes, 0, 0);
    let batch = Builder::new().record(record).build();
    assert_corrupt(&batch, 12, Reason::InvalidUtf8);
}

#[test]
fn a_long_valid_utf8_text_round_trips_past_the_scan_boundary() {
    let long = "\u{c4}".repeat(64);
    let record = common::text(1, [1.0, 2.0, 3.0], 4.0, 5.0, &long);
    let batch = Builder::new().record(record).build();
    let reader = BatchReader::new(&batch).expect("parses");
    match reader.records().next().expect("one record") {
        Ok(Record::Text(t)) => assert_eq!(t.text, long),
        other => panic!("expected a Text, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// What happens after a refusal
// ---------------------------------------------------------------------------

#[test]
fn the_iterator_stops_dead_after_an_error() {
    let batch = Builder::new()
        .record(canonical(3))
        .record(frame_raw(3, 0, 4, &[0u8; 16]))
        .record(canonical(5))
        .build();
    let reader = BatchReader::new(&batch).expect("parses");
    let mut records = reader.records();
    assert!(records.next().expect("first").is_ok());
    assert!(records.next().expect("second").is_err());
    for _ in 0..4 {
        assert!(
            records.next().is_none(),
            "a refused batch has nothing more to say, forever"
        );
    }
}

#[test]
fn an_error_renders_with_its_offset_and_its_reason() {
    let batch = Builder::new()
        .record(frame_raw(3, 0, 4, &[0u8; 16]))
        .build();
    let text = first_error(&batch).to_string();
    assert!(text.contains("12"), "the message names the offset: {text}");
    assert!(!text.is_empty());
}
