//! The whole safe API walked end to end against the real pinned library.
//!
//! There is no stub anywhere in here. ABI.md's synthetic document is the
//! library's own conformance input: eight ASCII bytes and two optional counts,
//! recognised by both open calls, emitting at least one of every record type
//! plus a forward probe. Its numbers are contractual, so every assertion below
//! is on an exact value rather than on "it did not crash".
//!
//! The rule the whole file rests on: the k-th `f64` in a record's payload,
//! counting from zero after the sixteen byte geometry prologue, is
//! `100 * type + k + 0.25`, and `item_handle` is `1000000 + type`.
#![cfg(acadsharp_linked)]

use acadsharp_rs::{Decoder, Document, Item, ItemHandle, Limits, Primitive, ViewKind};

/// `VIPRSSYN` with a view count and a primitive count, little-endian, which is
/// exactly how ABI.md spells the optional counts.
fn synthetic(views: u32, primitives: u32) -> Vec<u8> {
    let mut bytes = b"VIPRSSYN".to_vec();
    bytes.extend_from_slice(&views.to_le_bytes());
    bytes.extend_from_slice(&primitives.to_le_bytes());
    bytes
}

fn decoder() -> Decoder {
    Decoder::new().expect("the handshake passes against the pinned archive")
}

/// Every `f64` a primitive carries, in the order WIRE.md lists them, which is
/// the order the probe numbers them in.
fn doubles_of(primitive: &Primitive) -> Vec<f64> {
    match primitive {
        Primitive::Line(l) => l.start.iter().chain(l.end.iter()).copied().collect(),
        Primitive::Polyline(p) | Primitive::Polygon(p) => {
            let mut out = p.normal.to_vec();
            out.extend(p.vertices.iter().flatten().copied());
            out.extend(p.bulges.iter().copied());
            out
        }
        Primitive::Arc(a) => {
            let mut out = a.centre.to_vec();
            out.push(a.radius);
            out.push(a.start_angle);
            out.push(a.end_angle);
            out.extend_from_slice(&a.normal);
            out
        }
        Primitive::Circle(c) => {
            let mut out = c.centre.to_vec();
            out.push(c.radius);
            out.extend_from_slice(&c.normal);
            out
        }
        Primitive::Ellipse(e) => {
            let mut out = e.centre.to_vec();
            out.extend_from_slice(&e.major_axis);
            out.push(e.ratio);
            out.push(e.start_param);
            out.push(e.end_param);
            out.extend_from_slice(&e.normal);
            out
        }
        Primitive::Spline(s) => {
            let mut out = s.knots.clone();
            out.extend(s.controls.iter().flatten().copied());
            out.extend(s.weights.iter().copied());
            out
        }
        Primitive::Text(t) => {
            let mut out = t.position.to_vec();
            out.push(t.height);
            out.push(t.rotation);
            out
        }
        other => panic!("a primitive variant this helper has not been taught: {other:?}"),
    }
}

fn collect(bytes: &[u8], view: u32) -> Vec<Item> {
    let decoder = decoder();
    let document = Document::open_bytes(&decoder, bytes, &Limits::new()).expect("it opens");
    let mut stream = document.decode(view).expect("it decodes");
    let items: Vec<Item> = (&mut stream)
        .map(|item| item.expect("every item of the synthetic document parses"))
        .collect();
    assert!(stream.is_complete(), "the stream ran to its DocumentEnd");
    items
}

#[test]
fn capabilities_are_what_the_pinned_build_reports() {
    let decoder = decoder();
    let caps = decoder.capabilities();
    assert_eq!(caps.abi_version(), acadsharp_rs::EXPECTED_ABI_VERSION);
    assert_eq!(caps.wire_version(), acadsharp_rs::EXPECTED_WIRE_VERSION);
    assert_eq!(caps.dwg_version_min(), 1014);
    assert_eq!(caps.dwg_version_max(), 1032);
    assert!(caps.supports_block_expansion());
    assert!(caps.supports_warnings());
    assert_eq!(
        caps.acadsharp_version(),
        "3.7.1",
        "the pinned backing reader, read through the caller-buffer convention"
    );
}

#[test]
fn the_views_are_the_two_the_synthetic_document_describes() {
    let decoder = decoder();
    let document =
        Document::open_bytes(&decoder, &synthetic(2, 3), &Limits::new()).expect("it opens");

    assert_eq!(document.view_count().expect("a count"), 2);
    let views = document.views().expect("the views");
    assert_eq!(views.len(), 2);

    assert_eq!(views[0].index(), 0);
    assert_eq!(views[0].name(), "Model");
    assert_eq!(views[0].kind(), ViewKind::Model);
    assert_eq!(views[0].raw_kind(), 0);
    let extents = views[0].extents().expect("view 0 has a right way round box");
    assert_eq!(
        [
            extents.min_x,
            extents.min_y,
            extents.max_x,
            extents.max_y
        ],
        [-100.25, -50.5, 100.75, 50.125],
        "four different numbers, none round and none symmetric with another"
    );

    assert_eq!(views[1].index(), 1);
    assert_eq!(views[1].name(), "Layout1");
    assert_eq!(
        views[1].kind(),
        ViewKind::Layout,
        "spelled layout, the way both contracts spell it"
    );
    assert_eq!(views[1].raw_kind(), 1);

    // An index past the end is a refusal rather than a panic or a zeroed view.
    assert!(document.view(2).is_err());
}

#[test]
fn the_stream_carries_every_record_type_in_wire_order() {
    let items = collect(&synthetic(1, 12), 0);
    let types: Vec<u16> = items.iter().map(Item::record_type).collect();
    assert_eq!(
        types,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 3, 4, 5, 32512, 12, 13],
        "one of every record type, a warning, and the forward probe"
    );

    // Warnings are inline in the stream rather than a side channel, because
    // WIRE.md tells a consumer to read EMPTY_VIEW by what sits beside it.
    let Item::Warning(warning) = &items[10] else {
        panic!("record 11 is the warning")
    };
    assert_eq!(warning.code.get(), 1100);
    assert!(
        warning.code.is_backend_allocated(),
        "1100 is in the backend's range, which is why the code is a newtype and not a closed enum"
    );
    assert_eq!(warning.message, "VIPRS-WARNING-PROBE");
    assert_eq!(warning.entity, Some(ItemHandle::new(1_000_011)));

    // The forward probe survives as an unknown type, skipped by its length.
    let Item::Unknown {
        record_type,
        payload,
    } = &items[14]
    else {
        panic!("record 15 is the forward probe")
    };
    assert_eq!(*record_type, 32512);
    assert_eq!(payload.len(), 16);
}

#[test]
fn the_k_th_double_of_every_primitive_is_one_hundred_times_its_type_plus_k() {
    let items = collect(&synthetic(1, 12), 0);
    let mut checked = 0usize;
    for item in &items {
        let Item::Primitive(primitive) = item else {
            continue;
        };
        let t = primitive.record_type();
        let got = doubles_of(primitive);
        assert!(!got.is_empty(), "a type {t} record carries doubles");
        for (k, value) in got.iter().enumerate() {
            let want = 100.0 * f64::from(t) + k as f64 + 0.25;
            assert_eq!(
                *value, want,
                "the f64 at index {k} of a type {t} record should be 100 * {t} + {k} + 0.25"
            );
            checked += 1;
        }
        assert_eq!(
            primitive.origin().item_handle,
            Some(ItemHandle::new(1_000_000 + u64::from(t))),
            "item_handle is 1000000 + type"
        );
        assert!(
            !primitive.origin().from_expanded_insert,
            "the probe sets no flags"
        );
    }
    assert!(checked >= 60, "checked {checked} doubles, expected dozens");

    // Print one, so the convention the whole file rests on is visible in the
    // output rather than only inside an assertion.
    let line = items
        .iter()
        .find_map(|item| match item {
            Item::Primitive(p @ Primitive::Line(_)) => Some(p),
            _ => None,
        })
        .expect("the stream carries a Line");
    let first = doubles_of(line)[0];
    println!("the first f64 of the Line record reads back as {first}");
    assert_eq!(first, 100.0 * 3.0 + 0.0 + 0.25);
    assert_eq!(first, 300.25);
}

#[test]
fn the_variable_length_primitives_carry_the_wires_own_fields() {
    let items = collect(&synthetic(1, 12), 0);

    let Item::Primitive(Primitive::Polyline(polyline)) = &items[3] else {
        panic!("record 4 is the polyline")
    };
    assert_eq!(polyline.vertices.len(), 4);
    assert_eq!(
        polyline.bulges.len(),
        4,
        "one bulge per vertex, carried verbatim rather than tessellated away"
    );
    assert!(!polyline.closed);
    assert_eq!(polyline.normal, [400.25, 401.25, 402.25]);

    let Item::Primitive(Primitive::Polygon(polygon)) = &items[8] else {
        panic!("record 9 is the polygon")
    };
    assert!(polygon.closed, "a Polygon is record 4's payload with closed set");
    assert_eq!(polygon.vertices.len(), 3);
    assert_eq!(polygon.bulges.len(), 3);

    let Item::Primitive(Primitive::Spline(spline)) = &items[7] else {
        panic!("record 8 is the spline")
    };
    assert_eq!(spline.degree, 3);
    assert_eq!(spline.knots.len(), 8);
    assert_eq!(spline.controls.len(), 4);
    assert_eq!(spline.weights.len(), 4);

    let Item::Primitive(Primitive::Text(text)) = &items[9] else {
        panic!("record 10 is the text")
    };
    assert!(text.text.starts_with("VIPRS-TEXT-PROBE-"));
    assert_eq!(text.text.len(), 22, "the probe string is not ASCII only");
}

#[test]
fn the_totals_are_surfaced_because_they_are_the_only_completeness_proof() {
    let decoder = decoder();
    let document =
        Document::open_bytes(&decoder, &synthetic(1, 12), &Limits::new()).expect("it opens");
    let mut stream = document.decode(0).expect("it decodes");

    assert_eq!(stream.document_end(), None, "nothing is known before the walk");

    let items: Vec<Item> = (&mut stream).map(|i| i.expect("it parses")).collect();

    let begin = stream.document_begin().expect("a DocumentBegin went past");
    assert_eq!(begin.view_count, 1);
    assert_eq!(begin.drawing_version, 1032);

    let view_end = stream.view_end().expect("a ViewEnd went past");
    assert_eq!(view_end.view_index, 0);
    assert_eq!(view_end.record_count, 15);

    let end = stream.document_end().expect("a DocumentEnd went past");
    assert_eq!(end.total_records, 17);
    assert_eq!(end.warning_count, 1);

    assert_eq!(stream.records_seen(), 17);
    assert_eq!(stream.records_seen(), items.len() as u64);
    assert_eq!(stream.warnings_seen(), end.warning_count);
    assert!(
        stream.is_complete(),
        "a stream whose DocumentEnd total matches what it walked is the only proof of \
         a decode that was not truncated"
    );
}

#[test]
fn a_second_view_decodes_on_its_own_handle() {
    let items = collect(&synthetic(2, 3), 1);
    let Item::ViewBegin(view) = &items[1] else {
        panic!("the second record is the ViewBegin")
    };
    assert_eq!(view.index(), 1);
    assert_eq!(view.name(), "Layout1");
    assert_eq!(view.kind(), ViewKind::Layout);
    assert_eq!(
        items.last().map(Item::record_type),
        Some(13),
        "one decode handle produces one ViewBegin/ViewEnd pair and one DocumentEnd"
    );
}

#[test]
fn a_path_opens_the_same_document_the_bytes_do() {
    let dir = std::env::temp_dir().join(format!("acadsharp-rs-h31-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("synthetic.dwg");
    std::fs::write(&path, synthetic(1, 12)).expect("writing the probe file");

    let decoder = decoder();
    let document =
        Document::open_path(&decoder, &path, &Limits::new()).expect("the path route opens it");
    let mut stream = document.decode(0).expect("it decodes");
    let types: Vec<u16> = (&mut stream)
        .map(|item| item.expect("it parses").record_type())
        .collect();
    assert_eq!(
        types,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 3, 4, 5, 32512, 12, 13],
        "the sniff happens on both routes, which is the half a second implementation gets wrong"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
}

#[test]
fn an_empty_input_is_refused_here_rather_than_forwarded() {
    let decoder = decoder();
    assert!(
        Document::open_bytes(&decoder, b"", &Limits::new()).is_err(),
        "a zero length is a caller mistake everywhere on this boundary"
    );
    assert!(Document::open_path(&decoder, "", &Limits::new()).is_err());
}

#[test]
fn a_drawing_this_build_cannot_read_is_refused_with_the_range_to_look_at() {
    // Not a DWG and not the synthetic magic, so the library has to say what it
    // does with bytes it cannot read at all.
    let decoder = decoder();
    let outcome = Document::open_bytes(&decoder, b"not a drawing at all, sorry", &Limits::new());
    let error = outcome.expect_err("that is not a drawing");
    println!("opening nonsense bytes gives {error}");
    assert!(
        !matches!(error, acadsharp_rs::Error::Native(_)),
        "whatever it is, it has a name in this crate: {error:?}"
    );
}
