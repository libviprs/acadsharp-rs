//! A builder that writes well formed VACB batches from a record list.
//!
//! Every test in this crate constructs exactly the corruption it names and
//! nothing else, which is only possible if the well formed case comes from one
//! place. So this module writes the bytes and the tests bend them.
//!
//! The numbers it writes are the same ones the real library writes into its
//! `VIPRSSYN` synthetic document, `100 * type + k + 0.25` for the k-th `f64`
//! after the geometry prologue and `1000000 + type` for the handle, so the
//! round trip tests and the golden tests share one expectation function.

#![allow(dead_code)]

use acadsharp_rs::batch::{BatchReader, Record};

pub const MAGIC: [u8; 4] = *b"VACB";
pub const WIRE_VERSION: u16 = 2;
pub const BATCH_HEADER_LEN: usize = 12;

/// Rounds up to a multiple of four, which is what the wire pads strings to.
#[must_use]
pub fn pad4(v: usize) -> usize {
    (v + 3) & !3
}

/// The k-th `f64` of a record of this type, as `VIPRSSYN` writes it.
#[must_use]
pub fn probe_value(record_type: u16, k: usize) -> f64 {
    100.0 * f64::from(record_type) + k as f64 + 0.25
}

/// The `item_handle` a record of this type carries in `VIPRSSYN`.
#[must_use]
pub fn probe_handle(record_type: u16) -> u64 {
    1_000_000 + u64::from(record_type)
}

/// `count` consecutive probe values for a record of this type.
#[must_use]
pub fn probe_values(record_type: u16, count: usize) -> Vec<f64> {
    (0..count).map(|k| probe_value(record_type, k)).collect()
}

// ---------------------------------------------------------------------------
// Batch framing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Builder {
    magic: [u8; 4],
    wire_version: u16,
    flags: u16,
    payload: Vec<u8>,
    declared_payload_length: Option<u32>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            magic: MAGIC,
            wire_version: WIRE_VERSION,
            flags: 1,
            payload: Vec::new(),
            declared_payload_length: None,
        }
    }

    #[must_use]
    pub fn magic(mut self, magic: [u8; 4]) -> Self {
        self.magic = magic;
        self
    }

    #[must_use]
    pub fn wire_version(mut self, version: u16) -> Self {
        self.wire_version = version;
        self
    }

    #[must_use]
    pub fn flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    #[must_use]
    pub fn record(mut self, record: Vec<u8>) -> Self {
        self.payload.extend_from_slice(&record);
        self
    }

    #[must_use]
    pub fn records(mut self, records: &[Vec<u8>]) -> Self {
        for r in records {
            self.payload.extend_from_slice(r);
        }
        self
    }

    /// Appends bytes that are not a framed record, for the tests that need a
    /// payload ending in something a record header cannot be read out of.
    #[must_use]
    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.payload.extend_from_slice(bytes);
        self
    }

    /// Declares a `payload_length` other than the bytes actually written.
    #[must_use]
    pub fn declared_payload_length(mut self, n: u32) -> Self {
        self.declared_payload_length = Some(n);
        self
    }

    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(BATCH_HEADER_LEN + self.payload.len());
        out.extend_from_slice(&self.magic);
        out.extend_from_slice(&self.wire_version.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        let declared = self
            .declared_payload_length
            .unwrap_or(u32::try_from(self.payload.len()).expect("test payload fits a u32"));
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Wraps a payload in a record header whose `length` is the truth.
#[must_use]
pub fn frame(record_type: u16, payload: &[u8]) -> Vec<u8> {
    let length = 8 + payload.len();
    assert_eq!(
        length % 4,
        0,
        "the builder wrote a record of length {length}, which is not a multiple of four"
    );
    frame_raw(
        record_type,
        0,
        u32::try_from(length).expect("test record fits a u32"),
        payload,
    )
}

/// Wraps a payload in a record header carrying whatever `reserved` and
/// `length` the caller asks for, however untrue.
#[must_use]
pub fn frame_raw(record_type: u16, reserved: u16, length: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&record_type.to_le_bytes());
    out.extend_from_slice(&reserved.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn prologue(handle: u64, flags: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn push_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_f64s(out: &mut Vec<u8>, vs: &[f64]) {
    for v in vs {
        push_f64(out, *v);
    }
}

/// Pads a string's bytes out to a multiple of four with `fill`.
fn push_padded(out: &mut Vec<u8>, bytes: &[u8], fill: u8) {
    out.extend_from_slice(bytes);
    out.resize(out.len() + (pad4(bytes.len()) - bytes.len()), fill);
}

// ---------------------------------------------------------------------------
// Records, one constructor per type
// ---------------------------------------------------------------------------

#[must_use]
pub fn document_begin(view_count: u32, drawing_version: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&view_count.to_le_bytes());
    p.extend_from_slice(&drawing_version.to_le_bytes());
    p.extend_from_slice(&0u64.to_le_bytes());
    frame(1, &p)
}

#[must_use]
pub fn view_begin(index: u32, kind: u32, extents: [f64; 4], item_count: u64, name: &str) -> Vec<u8> {
    view_begin_raw(index, kind, extents, item_count, name.as_bytes(), 0, 0)
}

/// The same record with the reserved field and the string padding under the
/// caller's control, so the "do not refuse this" tests can dirty both.
#[must_use]
pub fn view_begin_raw(
    index: u32,
    kind: u32,
    extents: [f64; 4],
    item_count: u64,
    name: &[u8],
    reserved0: u32,
    pad_fill: u8,
) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&index.to_le_bytes());
    p.extend_from_slice(&kind.to_le_bytes());
    push_f64s(&mut p, &extents);
    p.extend_from_slice(&item_count.to_le_bytes());
    p.extend_from_slice(&u32::try_from(name.len()).expect("name fits a u32").to_le_bytes());
    p.extend_from_slice(&reserved0.to_le_bytes());
    push_padded(&mut p, name, pad_fill);
    frame(2, &p)
}

#[must_use]
pub fn line(handle: u64, coords: [f64; 6]) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    push_f64s(&mut p, &coords);
    frame(3, &p)
}

#[must_use]
pub fn polyline(
    handle: u64,
    closed: bool,
    normal: [f64; 3],
    vertices: &[[f64; 3]],
    bulges: &[f64],
) -> Vec<u8> {
    polyline_typed(4, handle, closed, normal, vertices, bulges)
}

#[must_use]
pub fn polygon(handle: u64, normal: [f64; 3], vertices: &[[f64; 3]], bulges: &[f64]) -> Vec<u8> {
    polyline_typed(9, handle, true, normal, vertices, bulges)
}

#[must_use]
pub fn polyline_typed(
    record_type: u16,
    handle: u64,
    closed: bool,
    normal: [f64; 3],
    vertices: &[[f64; 3]],
    bulges: &[f64],
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    p.extend_from_slice(&u32::try_from(vertices.len()).expect("fits").to_le_bytes());
    p.extend_from_slice(&u32::from(closed).to_le_bytes());
    p.extend_from_slice(&u32::try_from(bulges.len()).expect("fits").to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    push_f64s(&mut p, &normal);
    for v in vertices {
        push_f64s(&mut p, v);
    }
    push_f64s(&mut p, bulges);
    frame(record_type, &p)
}

/// A polyline whose declared counts need not agree with the bytes that follow.
#[must_use]
pub fn polyline_raw(
    handle: u64,
    point_count: u32,
    closed: u32,
    bulge_count: u32,
    reserved1: u32,
    tail: &[f64],
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    p.extend_from_slice(&point_count.to_le_bytes());
    p.extend_from_slice(&closed.to_le_bytes());
    p.extend_from_slice(&bulge_count.to_le_bytes());
    p.extend_from_slice(&reserved1.to_le_bytes());
    push_f64s(&mut p, tail);
    frame(4, &p)
}

/// The `point_count` that makes `64 + 24n + 8bc` come out to exactly the
/// declared 72 when the arithmetic is done in `u32`, and to 68,719,476,808
/// when it is done in `u64`. No random mutation reaches this value.
pub const WRAP_POINT_COUNT: u32 = 2_863_311_531;

/// A 72 byte `Polyline` claiming 2,863,311,531 vertices, which is 68 GB of
/// them.
#[must_use]
pub fn polyline_u32_wrap() -> Vec<u8> {
    // Three doubles of normal plus the eight bytes a wrapped parser reads as
    // the first half of a vertex. That is 64 bytes of payload, so 72 in all.
    polyline_raw(
        probe_handle(4),
        WRAP_POINT_COUNT,
        0,
        0,
        0,
        &[400.25, 401.25, 402.25, 403.25],
    )
}

#[must_use]
pub fn arc(
    handle: u64,
    centre: [f64; 3],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    normal: [f64; 3],
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    push_f64s(&mut p, &centre);
    push_f64(&mut p, radius);
    push_f64(&mut p, start_angle);
    push_f64(&mut p, end_angle);
    push_f64s(&mut p, &normal);
    frame(5, &p)
}

#[must_use]
pub fn circle(handle: u64, centre: [f64; 3], radius: f64, normal: [f64; 3]) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    push_f64s(&mut p, &centre);
    push_f64(&mut p, radius);
    push_f64s(&mut p, &normal);
    frame(6, &p)
}

#[must_use]
pub fn ellipse(
    handle: u64,
    centre: [f64; 3],
    major: [f64; 3],
    ratio: f64,
    params: [f64; 2],
    normal: [f64; 3],
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    push_f64s(&mut p, &centre);
    push_f64s(&mut p, &major);
    push_f64(&mut p, ratio);
    push_f64s(&mut p, &params);
    push_f64s(&mut p, &normal);
    frame(7, &p)
}

#[must_use]
pub fn spline(
    handle: u64,
    degree: u32,
    flags: u32,
    knots: &[f64],
    controls: &[[f64; 3]],
    weights: &[f64],
) -> Vec<u8> {
    spline_raw(
        handle,
        degree,
        flags,
        u32::try_from(knots.len()).expect("fits"),
        u32::try_from(controls.len()).expect("fits"),
        u32::try_from(weights.len()).expect("fits"),
        knots,
        controls,
        weights,
    )
}

/// A spline whose declared counts need not agree with the arrays that follow.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn spline_raw(
    handle: u64,
    degree: u32,
    flags: u32,
    knot_count: u32,
    control_count: u32,
    weight_count: u32,
    knots: &[f64],
    controls: &[[f64; 3]],
    weights: &[f64],
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    p.extend_from_slice(&degree.to_le_bytes());
    p.extend_from_slice(&flags.to_le_bytes());
    p.extend_from_slice(&knot_count.to_le_bytes());
    p.extend_from_slice(&control_count.to_le_bytes());
    p.extend_from_slice(&weight_count.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    push_f64s(&mut p, knots);
    for c in controls {
        push_f64s(&mut p, c);
    }
    push_f64s(&mut p, weights);
    frame(8, &p)
}

#[must_use]
pub fn text(handle: u64, position: [f64; 3], height: f64, rotation: f64, text: &str) -> Vec<u8> {
    text_raw(handle, position, height, rotation, text.as_bytes(), 0, 0)
}

#[must_use]
pub fn text_raw(
    handle: u64,
    position: [f64; 3],
    height: f64,
    rotation: f64,
    bytes: &[u8],
    reserved1: u32,
    pad_fill: u8,
) -> Vec<u8> {
    let mut p = prologue(handle, 0);
    push_f64s(&mut p, &position);
    push_f64(&mut p, height);
    push_f64(&mut p, rotation);
    p.extend_from_slice(&u32::try_from(bytes.len()).expect("fits").to_le_bytes());
    p.extend_from_slice(&reserved1.to_le_bytes());
    push_padded(&mut p, bytes, pad_fill);
    frame(10, &p)
}

#[must_use]
pub fn warning(code: u32, handle: u64, message: &str) -> Vec<u8> {
    warning_raw(code, handle, message.as_bytes(), 0, 0, 0)
}

#[must_use]
pub fn warning_raw(
    code: u32,
    handle: u64,
    message: &[u8],
    reserved0: u32,
    reserved1: u32,
    pad_fill: u8,
) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&code.to_le_bytes());
    p.extend_from_slice(&reserved0.to_le_bytes());
    p.extend_from_slice(&handle.to_le_bytes());
    p.extend_from_slice(&u32::try_from(message.len()).expect("fits").to_le_bytes());
    p.extend_from_slice(&reserved1.to_le_bytes());
    push_padded(&mut p, message, pad_fill);
    frame(11, &p)
}

#[must_use]
pub fn view_end(view_index: u32, record_count: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&view_index.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&record_count.to_le_bytes());
    frame(12, &p)
}

#[must_use]
pub fn document_end(total_records: u64, warning_count: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&total_records.to_le_bytes());
    p.extend_from_slice(&warning_count.to_le_bytes());
    frame(13, &p)
}

/// The forward probe the real library emits, bytes included.
#[must_use]
pub fn forward_probe() -> Vec<u8> {
    let mut p = b"v2-only-recor".to_vec();
    p.resize(16, 0);
    frame(32512, &p)
}

// ---------------------------------------------------------------------------
// Canonical records, carrying exactly the numbers `VIPRSSYN` carries
// ---------------------------------------------------------------------------

/// One record of each geometry type, with `VIPRSSYN`'s own numbers in it.
#[must_use]
pub fn canonical(record_type: u16) -> Vec<u8> {
    let h = probe_handle(record_type);
    let v = |k: usize| probe_value(record_type, k);
    match record_type {
        1 => document_begin(1, 1032),
        2 => view_begin(0, 0, [-100.25, -50.5, 100.75, 50.125], 12, "Model"),
        3 => line(h, [v(0), v(1), v(2), v(3), v(4), v(5)]),
        4 => polyline(
            h,
            false,
            [v(0), v(1), v(2)],
            &[
                [v(3), v(4), v(5)],
                [v(6), v(7), v(8)],
                [v(9), v(10), v(11)],
                [v(12), v(13), v(14)],
            ],
            &[v(15), v(16), v(17), v(18)],
        ),
        5 => arc(h, [v(0), v(1), v(2)], v(3), v(4), v(5), [v(6), v(7), v(8)]),
        6 => circle(h, [v(0), v(1), v(2)], v(3), [v(4), v(5), v(6)]),
        7 => ellipse(
            h,
            [v(0), v(1), v(2)],
            [v(3), v(4), v(5)],
            v(6),
            [v(7), v(8)],
            [v(9), v(10), v(11)],
        ),
        8 => spline(
            h,
            3,
            0,
            &[v(0), v(1), v(2), v(3), v(4), v(5), v(6), v(7)],
            &[
                [v(8), v(9), v(10)],
                [v(11), v(12), v(13)],
                [v(14), v(15), v(16)],
                [v(17), v(18), v(19)],
            ],
            &[v(20), v(21), v(22), v(23)],
        ),
        9 => polygon(
            h,
            [v(0), v(1), v(2)],
            &[
                [v(3), v(4), v(5)],
                [v(6), v(7), v(8)],
                [v(9), v(10), v(11)],
            ],
            &[v(12), v(13), v(14)],
        ),
        10 => text(h, [v(0), v(1), v(2)], v(3), v(4), "VIPRS-TEXT-PROBE-\u{c4}zzz"),
        11 => warning(1100, h, "VIPRS-WARNING-PROBE"),
        12 => view_end(0, 15),
        13 => document_end(17, 1),
        32512 => forward_probe(),
        other => panic!("no canonical record for type {other}"),
    }
}

/// Every type the synthetic document emits, in the order it emits them.
pub const CANONICAL_TYPES: [u16; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 32512, 12, 13];

// ---------------------------------------------------------------------------
// Reading back
// ---------------------------------------------------------------------------

/// Every `f64` a parsed record carries, in wire order, so a test can compare
/// against `probe_values` without naming twenty fields.
#[must_use]
pub fn doubles_of(record: &Record<'_>) -> Vec<f64> {
    match record {
        Record::Line(l) => l.start.iter().chain(l.end.iter()).copied().collect(),
        Record::Polyline(p) | Record::Polygon(p) => {
            let mut out = p.normal.to_vec();
            out.extend(p.vertices.iter().flatten());
            out.extend(p.bulges.iter());
            out
        }
        Record::Arc(a) => {
            let mut out = a.centre.to_vec();
            out.push(a.radius);
            out.push(a.start_angle);
            out.push(a.end_angle);
            out.extend_from_slice(&a.normal);
            out
        }
        Record::Circle(c) => {
            let mut out = c.centre.to_vec();
            out.push(c.radius);
            out.extend_from_slice(&c.normal);
            out
        }
        Record::Ellipse(e) => {
            let mut out = e.centre.to_vec();
            out.extend_from_slice(&e.major_axis);
            out.push(e.ratio);
            out.push(e.start_param);
            out.push(e.end_param);
            out.extend_from_slice(&e.normal);
            out
        }
        Record::Spline(s) => {
            let mut out: Vec<f64> = s.knots.iter().collect();
            out.extend(s.controls.iter().flatten());
            out.extend(s.weights.iter());
            out
        }
        Record::Text(t) => {
            let mut out = t.position.to_vec();
            out.push(t.height);
            out.push(t.rotation);
            out
        }
        _ => Vec::new(),
    }
}

/// The `item_handle` a parsed record carries, where it carries one.
#[must_use]
pub fn handle_of(record: &Record<'_>) -> Option<u64> {
    match record {
        Record::Line(l) => Some(l.prologue.item_handle),
        Record::Polyline(p) | Record::Polygon(p) => Some(p.prologue.item_handle),
        Record::Arc(a) => Some(a.prologue.item_handle),
        Record::Circle(c) => Some(c.prologue.item_handle),
        Record::Ellipse(e) => Some(e.prologue.item_handle),
        Record::Spline(s) => Some(s.prologue.item_handle),
        Record::Text(t) => Some(t.prologue.item_handle),
        Record::Warning(w) => Some(w.item_handle),
        _ => None,
    }
}

/// The wire number of a parsed record, for sequence assertions.
#[must_use]
pub fn type_of(record: &Record<'_>) -> u16 {
    match record {
        Record::DocumentBegin(_) => 1,
        Record::ViewBegin(_) => 2,
        Record::Line(_) => 3,
        Record::Polyline(_) => 4,
        Record::Arc(_) => 5,
        Record::Circle(_) => 6,
        Record::Ellipse(_) => 7,
        Record::Spline(_) => 8,
        Record::Polygon(_) => 9,
        Record::Text(_) => 10,
        Record::Warning(_) => 11,
        Record::ViewEnd(_) => 12,
        Record::DocumentEnd(_) => 13,
        Record::Unknown(u) => u.record_type,
        _ => u16::MAX,
    }
}

/// Walks a capture, which is a run of complete batches back to back, and
/// hands back every record in it. Panics on anything that does not parse,
/// because a golden capture that does not parse is the test failing.
#[must_use]
pub fn read_capture(bytes: &[u8]) -> Vec<Record<'_>> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let batch = BatchReader::new(&bytes[offset..])
            .unwrap_or_else(|e| panic!("the batch at {offset} does not parse: {e}"));
        for (i, record) in batch.records().enumerate() {
            out.push(
                record.unwrap_or_else(|e| {
                    panic!("record {i} of the batch at {offset} does not parse: {e}")
                }),
            );
        }
        offset += batch.total_len();
    }
    out
}
