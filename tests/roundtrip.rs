//! Every record type through the builder and back out of the reader, with
//! field equality, plus the whole "do not refuse this" list from the contract.
//!
//! Over strictness breaks on the next wire version and is as much a defect as
//! under strictness, so the second half of this file is as load bearing as the
//! first.

mod common;

use acadsharp_rs::batch::{BatchReader, Record};
use common::{
    Builder, CANONICAL_TYPES, canonical, doubles_of, handle_of, probe_handle, probe_value,
    probe_values, type_of,
};

fn read_one(bytes: &[u8]) -> Record<'_> {
    let batch = BatchReader::new(bytes).expect("the builder writes a parseable batch");
    let mut records = batch.records();
    let first = records
        .next()
        .expect("one record was written")
        .expect("the record parses");
    assert!(records.next().is_none(), "exactly one record was written");
    first
}

#[test]
fn every_record_type_round_trips() {
    let batch = Builder::new()
        .records(
            &CANONICAL_TYPES
                .iter()
                .map(|t| canonical(*t))
                .collect::<Vec<_>>(),
        )
        .build();
    let reader = BatchReader::new(&batch).expect("the batch parses");
    let records: Vec<_> = reader
        .records()
        .map(|r| r.expect("every canonical record parses"))
        .collect();

    let seen: Vec<u16> = records.iter().map(type_of).collect();
    assert_eq!(seen, CANONICAL_TYPES.to_vec());

    for record in &records {
        let t = type_of(record);
        if (3..=10).contains(&t) {
            assert_eq!(
                handle_of(record),
                Some(probe_handle(t)),
                "type {t} should carry handle {}",
                probe_handle(t)
            );
            let doubles = doubles_of(record);
            assert!(!doubles.is_empty(), "type {t} carries doubles");
            assert_eq!(
                doubles,
                probe_values(t, doubles.len()),
                "type {t} doubles are 100 * type + k + 0.25"
            );
        }
    }
}

#[test]
fn document_begin_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(1)).build()) {
        Record::DocumentBegin(d) => {
            assert_eq!(d.view_count, 1);
            assert_eq!(d.drawing_version, 1032);
        }
        other => panic!("expected a DocumentBegin, got {other:?}"),
    }
}

#[test]
fn view_begin_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(2)).build()) {
        Record::ViewBegin(v) => {
            assert_eq!(v.view_index, 0);
            assert_eq!(v.kind, 0);
            assert_eq!(v.item_count, 12);
            assert_eq!(v.name, "Model");
            let b = v.bounds.expect("a right way round box is Some");
            assert_eq!(
                [b.min_x, b.min_y, b.max_x, b.max_y],
                [-100.25, -50.5, 100.75, 50.125]
            );
        }
        other => panic!("expected a ViewBegin, got {other:?}"),
    }
}

#[test]
fn line_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(3)).build()) {
        Record::Line(l) => {
            assert_eq!(l.prologue.item_handle, 1_000_003);
            assert!(!l.prologue.from_expanded_insert());
            assert_eq!(l.start, [300.25, 301.25, 302.25]);
            assert_eq!(l.end, [303.25, 304.25, 305.25]);
        }
        other => panic!("expected a Line, got {other:?}"),
    }
}

#[test]
fn polyline_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(4)).build()) {
        Record::Polyline(p) => {
            assert_eq!(p.prologue.item_handle, 1_000_004);
            assert!(!p.closed);
            assert_eq!(p.normal, [400.25, 401.25, 402.25]);
            assert_eq!(p.vertices.len(), 4);
            assert_eq!(p.bulges.len(), 4);
            assert_eq!(p.vertices.get(0), Some([403.25, 404.25, 405.25]));
            assert_eq!(p.vertices.get(3), Some([412.25, 413.25, 414.25]));
            assert_eq!(p.vertices.get(4), None);
            assert_eq!(p.bulges.get(0), Some(415.25));
            assert_eq!(p.bulges.get(3), Some(418.25));
            assert_eq!(p.bulges.get(4), None);
        }
        other => panic!("expected a Polyline, got {other:?}"),
    }
}

#[test]
fn polygon_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(9)).build()) {
        Record::Polygon(p) => {
            assert_eq!(p.prologue.item_handle, 1_000_009);
            assert!(p.closed, "a Polygon is closed on the wire");
            assert_eq!(p.vertices.len(), 3);
            assert_eq!(p.bulges.len(), 3);
            assert_eq!(p.vertices.get(2), Some([909.25, 910.25, 911.25]));
        }
        other => panic!("expected a Polygon, got {other:?}"),
    }
}

#[test]
fn arc_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(5)).build()) {
        Record::Arc(a) => {
            assert_eq!(a.centre, [500.25, 501.25, 502.25]);
            assert_eq!(a.radius, 503.25);
            assert_eq!(a.start_angle, 504.25);
            assert_eq!(a.end_angle, 505.25);
            assert_eq!(a.normal, [506.25, 507.25, 508.25]);
        }
        other => panic!("expected an Arc, got {other:?}"),
    }
}

#[test]
fn circle_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(6)).build()) {
        Record::Circle(c) => {
            assert_eq!(c.centre, [600.25, 601.25, 602.25]);
            assert_eq!(c.radius, 603.25);
            assert_eq!(c.normal, [604.25, 605.25, 606.25]);
        }
        other => panic!("expected a Circle, got {other:?}"),
    }
}

#[test]
fn ellipse_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(7)).build()) {
        Record::Ellipse(e) => {
            assert_eq!(e.centre, [700.25, 701.25, 702.25]);
            assert_eq!(e.major_axis, [703.25, 704.25, 705.25]);
            assert_eq!(e.ratio, 706.25);
            assert_eq!(e.start_param, 707.25);
            assert_eq!(e.end_param, 708.25);
            assert_eq!(e.normal, [709.25, 710.25, 711.25]);
        }
        other => panic!("expected an Ellipse, got {other:?}"),
    }
}

#[test]
fn spline_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(8)).build()) {
        Record::Spline(s) => {
            assert_eq!(s.degree, 3);
            assert_eq!(s.flags, 0);
            assert_eq!(s.knots.len(), 8);
            assert_eq!(s.controls.len(), 4);
            assert_eq!(s.weights.len(), 4);
            assert_eq!(s.knots.get(0), Some(800.25));
            assert_eq!(s.knots.get(7), Some(807.25));
            assert_eq!(s.controls.get(0), Some([808.25, 809.25, 810.25]));
            assert_eq!(s.controls.get(3), Some([817.25, 818.25, 819.25]));
            assert_eq!(s.weights.get(3), Some(823.25));
        }
        other => panic!("expected a Spline, got {other:?}"),
    }
}

#[test]
fn text_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(10)).build()) {
        Record::Text(t) => {
            assert_eq!(t.position, [1000.25, 1001.25, 1002.25]);
            assert_eq!(t.height, 1003.25);
            assert_eq!(t.rotation, 1004.25);
            assert_eq!(t.text, "VIPRS-TEXT-PROBE-\u{c4}zzz");
            assert_eq!(t.text.len(), 22, "the multibyte char makes this 22 bytes");
        }
        other => panic!("expected a Text, got {other:?}"),
    }
}

#[test]
fn warning_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(11)).build()) {
        Record::Warning(w) => {
            assert_eq!(w.code, 1100);
            assert_eq!(w.item_handle, 1_000_011);
            assert_eq!(w.message, "VIPRS-WARNING-PROBE");
        }
        other => panic!("expected a Warning, got {other:?}"),
    }
}

#[test]
fn view_end_and_document_end_fields_round_trip() {
    match read_one(&Builder::new().record(canonical(12)).build()) {
        Record::ViewEnd(v) => {
            assert_eq!(v.view_index, 0);
            assert_eq!(v.record_count, 15);
        }
        other => panic!("expected a ViewEnd, got {other:?}"),
    }
    match read_one(&Builder::new().record(canonical(13)).build()) {
        Record::DocumentEnd(d) => {
            assert_eq!(d.total_records, 17);
            assert_eq!(d.warning_count, 1);
        }
        other => panic!("expected a DocumentEnd, got {other:?}"),
    }
}

#[test]
fn a_polyline_with_no_bulges_round_trips() {
    let record = common::polyline(
        1,
        true,
        [0.0, 0.0, 1.0],
        &[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
        &[],
    );
    match read_one(&Builder::new().record(record).build()) {
        Record::Polyline(p) => {
            assert!(p.closed);
            assert_eq!(p.vertices.len(), 2);
            assert_eq!(p.bulges.len(), 0);
            assert!(p.bulges.is_empty());
            assert_eq!(p.bulges.get(0), None);
        }
        other => panic!("expected a Polyline, got {other:?}"),
    }
}

#[test]
fn a_spline_with_no_weights_round_trips() {
    let record = common::spline(1, 2, 5, &[0.0, 1.0, 2.0], &[[1.0, 2.0, 3.0]], &[]);
    match read_one(&Builder::new().record(record).build()) {
        Record::Spline(s) => {
            assert_eq!(s.degree, 2);
            assert_eq!(s.flags, 5);
            assert_eq!(s.knots.len(), 3);
            assert_eq!(s.controls.len(), 1);
            assert!(s.weights.is_empty());
        }
        other => panic!("expected a Spline, got {other:?}"),
    }
}

#[test]
fn an_empty_batch_yields_no_records() {
    let batch = Builder::new().build();
    let reader = BatchReader::new(&batch).expect("an empty batch is legal");
    assert_eq!(reader.payload_len(), 0);
    assert_eq!(reader.total_len(), 12);
    assert_eq!(reader.records().count(), 0);
}

#[test]
fn a_batch_longer_than_its_own_bytes_is_not_read_past() {
    // Two batches back to back. Reading the first must stop at its own end.
    let first = Builder::new().record(canonical(3)).build();
    let second = Builder::new().record(canonical(5)).build();
    let mut stream = first.clone();
    stream.extend_from_slice(&second);

    let reader = BatchReader::new(&stream).expect("the first batch parses");
    assert_eq!(reader.total_len(), first.len());
    assert_eq!(reader.records().count(), 1);

    let next = BatchReader::new(&stream[reader.total_len()..]).expect("the second batch parses");
    assert_eq!(next.records().count(), 1);
}

#[test]
fn the_last_batch_flag_is_read_off_bit_zero() {
    let last = Builder::new().flags(1).build();
    assert!(BatchReader::new(&last).expect("parses").is_last());
    let more = Builder::new().flags(0).build();
    assert!(!BatchReader::new(&more).expect("parses").is_last());
}

// ---------------------------------------------------------------------------
// The "do NOT refuse this" list. Over strictness is a defect too.
// ---------------------------------------------------------------------------

#[test]
fn batch_flags_above_bit_zero_are_ignored_not_refused() {
    let batch = Builder::new().flags(0xFFFF).record(canonical(3)).build();
    let reader = BatchReader::new(&batch).expect("unknown flag bits are ignored");
    assert!(reader.is_last(), "bit 0 still means what it means");
    assert_eq!(reader.flags(), 0xFFFF, "the raw flags are surfaced verbatim");
    assert_eq!(reader.records().count(), 1);
}

#[test]
fn payload_reserved_fields_are_not_refused() {
    // reserved1 in a Text, reserved0 in a ViewBegin, both non zero.
    let dirty_text = common::text_raw(1, [1.0, 2.0, 3.0], 4.0, 5.0, b"hi", 0xDEAD_BEEF, 0);
    let dirty_view = common::view_begin_raw(0, 0, [0.0; 4], 1, b"Model", 0xFFFF_FFFF, 0);
    let dirty_warning = common::warning_raw(7, 1, b"x", 0x1234, 0x5678, 0);
    let batch = Builder::new()
        .records(&[dirty_text, dirty_view, dirty_warning])
        .build();
    let reader = BatchReader::new(&batch).expect("parses");
    assert_eq!(
        reader.records().filter(|r| r.is_ok()).count(),
        3,
        "reserved payload fields are not mine to police"
    );
}

#[test]
fn non_zero_string_padding_is_not_refused() {
    let text = common::text_raw(1, [1.0, 2.0, 3.0], 4.0, 5.0, b"hi", 0, 0xAA);
    let view = common::view_begin_raw(0, 0, [0.0; 4], 1, b"Model", 0, 0xBB);
    let warning = common::warning_raw(7, 1, b"x", 0, 0, 0xCC);
    let batch = Builder::new().records(&[text, view, warning]).build();
    let reader = BatchReader::new(&batch).expect("parses");
    let records: Vec<_> = reader.records().map(|r| r.expect("parses")).collect();
    match &records[0] {
        Record::Text(t) => assert_eq!(t.text, "hi", "the string stops at byte_len"),
        other => panic!("expected a Text, got {other:?}"),
    }
    match &records[1] {
        Record::ViewBegin(v) => assert_eq!(v.name, "Model"),
        other => panic!("expected a ViewBegin, got {other:?}"),
    }
}

#[test]
fn an_unknown_record_type_is_skipped_and_the_next_record_is_read() {
    let batch = Builder::new()
        .records(&[canonical(3), common::forward_probe(), canonical(5)])
        .build();
    let reader = BatchReader::new(&batch).expect("parses");
    let records: Vec<_> = reader.records().map(|r| r.expect("parses")).collect();
    assert_eq!(records.len(), 3);
    assert_eq!(type_of(&records[0]), 3);
    match &records[1] {
        Record::Unknown(u) => {
            assert_eq!(u.record_type, 32512);
            assert_eq!(u.payload.len(), 16);
            assert!(u.payload.starts_with(b"v2-only-recor"));
        }
        other => panic!("expected an Unknown, got {other:?}"),
    }
    match &records[2] {
        Record::Arc(a) => assert_eq!(a.centre[0], probe_value(5, 0)),
        other => panic!("expected the Arc after the probe, got {other:?}"),
    }
}

#[test]
fn an_unknown_warning_code_is_not_refused() {
    let batch = Builder::new()
        .record(common::warning(4_294_967_295, 0, "from some later backend"))
        .build();
    match read_one(&batch) {
        Record::Warning(w) => assert_eq!(w.code, u32::MAX),
        other => panic!("expected a Warning, got {other:?}"),
    }
}

#[test]
fn a_spline_whose_degree_disagrees_with_its_knot_count_is_accepted() {
    // A NURBS usually has knots == controls + degree + 1. The wire does not
    // promise it and the refusal list does not mention it, so I read it out
    // and leave the judgement to whoever knows what the curve is for.
    let record = common::spline(1, 9, 0, &[0.0, 1.0], &[[1.0, 2.0, 3.0]], &[]);
    match read_one(&Builder::new().record(record).build()) {
        Record::Spline(s) => {
            assert_eq!(s.degree, 9);
            assert_eq!(s.knots.len(), 2);
            assert_eq!(s.controls.len(), 1);
        }
        other => panic!("expected a Spline, got {other:?}"),
    }
}

#[test]
fn inverted_extents_read_as_no_bounds() {
    let record = common::view_begin(0, 2, [1e20, 1e20, -1e20, -1e20], 0, "Empty");
    match read_one(&Builder::new().record(record).build()) {
        Record::ViewBegin(v) => {
            assert!(
                v.bounds.is_none(),
                "the inverted box is not a rectangle and not a refusal"
            );
            assert_eq!(v.kind, 2);
            assert_eq!(v.extents, [1e20, 1e20, -1e20, -1e20]);
        }
        other => panic!("expected a ViewBegin, got {other:?}"),
    }
}

#[test]
fn a_non_finite_extent_is_not_refused() {
    // The finiteness rule is scoped to geometry. A ViewBegin is exempt, so
    // these bytes have to come back out rather than end the batch.
    let record = common::view_begin(0, 0, [f64::NAN, 0.0, f64::INFINITY, 1.0], 0, "Odd");
    match read_one(&Builder::new().record(record).build()) {
        Record::ViewBegin(v) => assert!(v.extents[0].is_nan()),
        other => panic!("expected a ViewBegin, got {other:?}"),
    }
}

#[test]
fn the_iteration_budget_allows_every_minimum_sized_record() {
    // `payload_length / 8 + 1` is exactly enough for a batch of nothing but
    // eight byte records. A budget one short would refuse this legal batch.
    let smallest = common::frame(32512, &[]);
    assert_eq!(smallest.len(), 8);
    let count = 512;
    let mut builder = Builder::new();
    for _ in 0..count {
        builder = builder.record(smallest.clone());
    }
    let batch = builder.build();
    let reader = BatchReader::new(&batch).expect("parses");
    assert_eq!(
        reader.records().filter(|r| r.is_ok()).count(),
        count,
        "the budget must not be tighter than the wire allows"
    );
}
