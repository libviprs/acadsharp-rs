//! The four captures in `tests/data` are real bytes, written by the published
//! `libviprs-dep` library running against its own `VIPRSSYN` synthetic
//! document. They are the library's own contractual probe output, a couple of
//! kilobytes in all, so they live in the repository rather than being
//! regenerated from an archive nobody has at test time.
//!
//! `ABI.md` makes their contents contractual: the k-th `f64` in a record
//! payload, counting from zero after the 16 byte geometry prologue, is
//! `100 * type + k + 0.25`, and `item_handle` is `1000000 + type`. That is what
//! turns "it parsed" into "it read the right field".

mod common;

use acadsharp_rs::batch::{BatchReader, Record};
use common::{doubles_of, handle_of, probe_handle, probe_values, read_capture, type_of};

const SYN_1V_12P: &[u8] = include_bytes!("data/syn_1v_12p.bin");
const SYN_2V_3P: &[u8] = include_bytes!("data/syn_2v_3p.bin");
const SYN_1V_1P: &[u8] = include_bytes!("data/syn_1v_1p.bin");
const SYN_3V_40P: &[u8] = include_bytes!("data/syn_3v_40p.bin");

const ALL: [(&str, &[u8]); 4] = [
    ("syn_1v_12p.bin", SYN_1V_12P),
    ("syn_2v_3p.bin", SYN_2V_3P),
    ("syn_1v_1p.bin", SYN_1V_1P),
    ("syn_3v_40p.bin", SYN_3V_40P),
];

#[test]
fn syn_1v_12p_carries_the_exact_record_sequence_i_verified_by_hand() {
    let records = read_capture(SYN_1V_12P);
    let types: Vec<u16> = records.iter().map(type_of).collect();
    assert_eq!(
        types,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 3, 4, 5, 32512, 12, 13],
        "every record type 1 to 13 plus the forward probe, in wire order"
    );

    match &records[0] {
        Record::DocumentBegin(d) => {
            assert_eq!(d.view_count, 1);
            assert_eq!(d.drawing_version, 1032);
        }
        other => panic!("expected a DocumentBegin, got {other:?}"),
    }
    match &records[1] {
        Record::ViewBegin(v) => {
            assert_eq!(v.view_index, 0);
            assert_eq!(v.kind, 0);
            assert_eq!(v.name, "Model");
            assert_eq!(v.item_count, 12);
            let b = v.bounds.expect("the Model view has a right way round box");
            assert_eq!(
                [b.min_x, b.min_y, b.max_x, b.max_y],
                [-100.25, -50.5, 100.75, 50.125]
            );
        }
        other => panic!("expected a ViewBegin, got {other:?}"),
    }
    match &records[3] {
        Record::Polyline(p) => {
            assert_eq!(p.vertices.len(), 4);
            assert!(!p.closed);
            assert_eq!(p.bulges.len(), 4);
        }
        other => panic!("expected a Polyline, got {other:?}"),
    }
    match &records[8] {
        Record::Polygon(p) => {
            assert_eq!(p.vertices.len(), 3);
            assert!(p.closed);
            assert_eq!(p.bulges.len(), 3);
        }
        other => panic!("expected a Polygon, got {other:?}"),
    }
    match &records[7] {
        Record::Spline(s) => {
            assert_eq!(s.degree, 3);
            assert_eq!(s.knots.len(), 8);
            assert_eq!(s.controls.len(), 4);
            assert_eq!(s.weights.len(), 4);
        }
        other => panic!("expected a Spline, got {other:?}"),
    }
    match &records[9] {
        Record::Text(t) => {
            assert_eq!(t.text.len(), 22);
            assert!(t.text.starts_with("VIPRS-TEXT-PROBE-"));
            assert!(
                t.text.contains('\u{c4}'),
                "the probe carries a multibyte character on purpose"
            );
        }
        other => panic!("expected a Text, got {other:?}"),
    }
    match &records[10] {
        Record::Warning(w) => {
            assert_eq!(w.code, 1100, "1100 is backend allocated, not a closed set");
            assert_eq!(w.item_handle, 1_000_011);
            assert_eq!(w.message, "VIPRS-WARNING-PROBE");
        }
        other => panic!("expected a Warning, got {other:?}"),
    }
    match &records[15] {
        Record::ViewEnd(v) => {
            assert_eq!(v.view_index, 0);
            assert_eq!(v.record_count, 15);
        }
        other => panic!("expected a ViewEnd, got {other:?}"),
    }
    match &records[16] {
        Record::DocumentEnd(d) => {
            assert_eq!(d.total_records, 17);
            assert_eq!(d.warning_count, 1);
            assert_eq!(
                d.total_records as usize,
                records.len(),
                "DocumentEnd counts itself and DocumentBegin"
            );
        }
        other => panic!("expected a DocumentEnd, got {other:?}"),
    }
}

#[test]
fn golden_payload_doubles_are_one_hundred_times_the_type_plus_the_index() {
    // The assertion that turns "it parsed" into "it read the right field".
    let records = read_capture(SYN_1V_12P);
    let mut checked = 0usize;
    for record in &records {
        let t = type_of(record);
        if !(3..=10).contains(&t) {
            continue;
        }
        let got = doubles_of(record);
        assert!(!got.is_empty(), "type {t} carries doubles");
        let want = probe_values(t, got.len());
        for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                g, w,
                "the f64 at index {k} of a type {t} record should be 100 * {t} + {k} + 0.25"
            );
            checked += 1;
        }
    }
    assert!(checked >= 60, "checked {checked} doubles, expected dozens");

    // Print one, so the number this whole convention rests on is visible in
    // the test output rather than only inside an assertion.
    let line = records
        .iter()
        .find(|r| type_of(r) == 3)
        .expect("the capture has a Line");
    let first = doubles_of(line)[0];
    println!("the first f64 of the Line record reads back as {first}");
    assert_eq!(first, 100.0 * 3.0 + 0.0 + 0.25);
    assert_eq!(first, 300.25);

    let spline = records
        .iter()
        .find(|r| type_of(r) == 8)
        .expect("the capture has a Spline");
    let weights = doubles_of(spline);
    println!(
        "the Spline's 21st f64 (its first weight) reads back as {}",
        weights[20]
    );
    assert_eq!(weights[20], 100.0 * 8.0 + 20.0 + 0.25);
    assert_eq!(weights[20], 820.25);
}

#[test]
fn golden_item_handles_are_one_million_plus_the_type() {
    for (name, bytes) in ALL {
        for record in &read_capture(bytes) {
            let t = type_of(record);
            if (3..=11).contains(&t) {
                assert_eq!(
                    handle_of(record),
                    Some(probe_handle(t)),
                    "{name}: a type {t} record should carry handle {}",
                    probe_handle(t)
                );
            }
        }
    }
}

#[test]
fn the_forward_probe_is_skipped_by_length_and_the_record_after_it_is_read() {
    // The probe's length matches no other record on purpose, so a consumer
    // that skipped by a size table would land in the middle of the ViewEnd.
    for (name, bytes) in ALL {
        let records = read_capture(bytes);
        let at = records
            .iter()
            .position(|r| type_of(r) == 32512)
            .unwrap_or_else(|| panic!("{name}: every capture carries a forward probe"));
        match &records[at] {
            Record::Unknown(u) => {
                assert_eq!(u.record_type, 32512);
                assert_eq!(u.payload.len(), 16);
                assert!(u.payload.starts_with(b"v2-only-recor"));
            }
            other => panic!("{name}: expected an Unknown, got {other:?}"),
        }
        match &records[at + 1] {
            Record::ViewEnd(_) => {}
            other => {
                panic!("{name}: the record after the probe should be a ViewEnd, got {other:?}")
            }
        }
    }
}

#[test]
fn every_capture_parses_end_to_end_with_nothing_left_over() {
    let expected: [(&str, usize, usize); 4] = [
        ("syn_1v_12p.bin", 1, 17),
        ("syn_2v_3p.bin", 2, 16),
        ("syn_1v_1p.bin", 1, 6),
        ("syn_3v_40p.bin", 3, 135),
    ];
    for ((name, bytes), (also, batches, records)) in ALL.into_iter().zip(expected) {
        assert_eq!(name, also);
        let mut offset = 0;
        let mut seen_batches = 0;
        let mut seen_records = 0;
        while offset < bytes.len() {
            let batch = BatchReader::new(&bytes[offset..])
                .unwrap_or_else(|e| panic!("{name} at {offset}: {e}"));
            assert!(batch.is_last(), "{name}: every capture batch sets bit 0");
            assert_eq!(batch.flags(), 1, "{name}: no other flag bit is set");
            for record in batch.records() {
                record.unwrap_or_else(|e| panic!("{name} at {offset}: {e}"));
                seen_records += 1;
            }
            offset += batch.total_len();
            seen_batches += 1;
        }
        assert_eq!(offset, bytes.len(), "{name}: nothing left over");
        assert_eq!(seen_batches, batches, "{name}: batch count");
        assert_eq!(seen_records, records, "{name}: record count");
    }
}

#[test]
fn the_view_and_document_totals_add_up_in_every_capture() {
    for (name, bytes) in ALL {
        let records = read_capture(bytes);
        let mut in_view = 0usize;
        let mut counting = false;
        let mut warnings = 0u64;
        for record in &records {
            if counting {
                in_view += 1;
            }
            match record {
                Record::ViewBegin(_) => {
                    counting = true;
                    in_view = 1;
                    warnings = 0;
                }
                Record::Warning(_) => warnings += 1,
                Record::ViewEnd(v) => {
                    counting = false;
                    assert_eq!(
                        v.record_count, in_view as u64,
                        "{name}: ViewEnd counts its own pair and everything between"
                    );
                }
                Record::DocumentEnd(d) => {
                    assert_eq!(
                        d.total_records,
                        in_view as u64 + 2,
                        "{name}: total_records is the view's count plus two"
                    );
                    assert_eq!(d.warning_count, warnings, "{name}: warning count");
                }
                _ => {}
            }
        }
    }
}

#[test]
fn every_golden_record_length_is_a_multiple_of_four_and_every_reserved_is_zero() {
    // I checked this by hand before writing the decoder. Asserting it here
    // means a capture swapped for one that does not hold it fails loudly
    // rather than quietly weakening every other test in this file.
    for (name, bytes) in ALL {
        let mut offset = 0;
        while offset < bytes.len() {
            let payload_length =
                u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().expect("4 bytes"))
                    as usize;
            let payload = &bytes[offset + 12..offset + 12 + payload_length];
            let mut pos = 0;
            while pos < payload.len() {
                let reserved =
                    u16::from_le_bytes(payload[pos + 2..pos + 4].try_into().expect("2 bytes"));
                let length =
                    u32::from_le_bytes(payload[pos + 4..pos + 8].try_into().expect("4 bytes"))
                        as usize;
                assert_eq!(reserved, 0, "{name}: reserved at {pos} is not zero");
                assert_eq!(
                    length % 4,
                    0,
                    "{name}: length at {pos} is not a multiple of four"
                );
                assert!(
                    length >= 8,
                    "{name}: length at {pos} is below its own header"
                );
                pos += length;
            }
            offset += 12 + payload_length;
        }
    }
}

#[test]
fn the_three_view_capture_walks_all_three_views_with_their_own_names_and_kinds() {
    let records = read_capture(SYN_3V_40P);
    let views: Vec<(u32, u32, String)> = records
        .iter()
        .filter_map(|r| match r {
            Record::ViewBegin(v) => Some((v.view_index, v.kind, v.name.to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(
        views,
        vec![
            (0, 0, "Model".to_owned()),
            (1, 1, "Layout1".to_owned()),
            (2, 1, "Layout2".to_owned()),
        ]
    );
}
