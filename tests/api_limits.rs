//! `Limits`: what a caller sets, what it means to leave a field alone, and
//! that the numbers actually reach the decoder.
//!
//! The round trip half runs everywhere. The half that proves a bound bites
//! needs the library, because a limit the crate stores and never forwards
//! looks exactly like a limit nothing exceeded.

use acadsharp_rs::Limits;

#[test]
fn a_fresh_limits_leaves_every_bound_to_the_library() {
    let limits = Limits::new();
    assert_eq!(limits, Limits::default());
    assert_eq!(limits.max_input_bytes(), None);
    assert_eq!(limits.max_entities(), None);
    assert_eq!(limits.max_string_bytes(), None);
    assert_eq!(limits.max_polyline_points(), None);
    assert_eq!(limits.max_block_depth(), None);
    assert_eq!(limits.max_output_bytes(), None);
}

#[test]
fn every_setter_round_trips_and_touches_nothing_else() {
    let base = Limits::new();

    let cases: [(Limits, &str); 6] = [
        (base.with_max_input_bytes(11), "max_input_bytes"),
        (base.with_max_entities(22), "max_entities"),
        (base.with_max_string_bytes(33), "max_string_bytes"),
        (base.with_max_polyline_points(44), "max_polyline_points"),
        (base.with_max_block_depth(55), "max_block_depth"),
        (base.with_max_output_bytes(66), "max_output_bytes"),
    ];

    let read = |l: &Limits| {
        [
            l.max_input_bytes(),
            l.max_entities(),
            l.max_string_bytes(),
            l.max_polyline_points(),
            l.max_block_depth(),
            l.max_output_bytes(),
        ]
    };

    for (index, (limits, name)) in cases.iter().enumerate() {
        let fields = read(limits);
        for (other, value) in fields.iter().enumerate() {
            if other == index {
                assert_eq!(
                    *value,
                    Some((index as u64 + 1) * 11),
                    "{name} should hold what was set"
                );
            } else {
                assert_eq!(*value, None, "setting {name} moved field {other} as well");
            }
        }
    }

    // All six at once, because a builder that overwrites its own earlier call
    // is a builder that passes every single field test.
    let all = Limits::new()
        .with_max_input_bytes(1)
        .with_max_entities(2)
        .with_max_string_bytes(3)
        .with_max_polyline_points(4)
        .with_max_block_depth(5)
        .with_max_output_bytes(6);
    assert_eq!(
        read(&all),
        [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]
    );
}

#[cfg(acadsharp_linked)]
mod against_the_library {
    use acadsharp_rs::{Decoder, Document, Error, Limits};

    /// One view, twelve primitives. The `Polyline` in it carries four
    /// vertices, which is what the bound below is set under.
    const SYNTHETIC: &[u8] = b"VIPRSSYN\x01\x00\x00\x00\x0c\x00\x00\x00";

    #[test]
    fn an_all_default_limits_is_one_the_library_accepts() {
        let decoder = Decoder::new().expect("the handshake passes against the pinned archive");
        let document = Document::open_bytes(&decoder, SYNTHETIC, &Limits::default())
            .expect("an all-zero limits means `every bound is yours to pick`");
        assert!(document.view_count().expect("a view count") >= 1);
    }

    #[test]
    fn a_block_depth_past_the_boundarys_own_field_is_refused_and_not_truncated() {
        // `max_block_depth` is the one bound the header declares as a
        // `uint32_t` while every bound up here is a `u64`, so that a later
        // widening of the field is not a change to the public API. The
        // narrowing has to be a refusal: `(1 << 32) + 64` truncated is 64,
        // which is the documented default, so the failure would look exactly
        // like a caller who never set the bound at all.
        let decoder = Decoder::new().expect("the handshake passes");

        // The positive control. The largest value the field can hold goes
        // across, so a refusal below is the narrowing check and not the
        // library disliking a large bound.
        let at_the_edge = Limits::new().with_max_block_depth(u64::from(u32::MAX));
        assert!(
            Document::open_bytes(&decoder, SYNTHETIC, &at_the_edge).is_ok(),
            "u32::MAX fits the field and has to be accepted"
        );

        for past in [u64::from(u32::MAX) + 1, u64::MAX] {
            let bounded = Limits::new().with_max_block_depth(past);
            assert_eq!(
                Document::open_bytes(&decoder, SYNTHETIC, &bounded).err(),
                Some(Error::InvalidArgument),
                "a block depth of {past} does not fit a uint32_t, and truncating it would \
                 silently run the decode under a bound the caller never asked for"
            );
        }
    }

    #[test]
    fn a_polyline_bound_under_the_probes_own_vertex_count_ends_the_decode() {
        let decoder = Decoder::new().expect("the handshake passes");

        // The positive control first. Without the bound the same stream runs
        // to its DocumentEnd, so a failure below is the bound biting rather
        // than the document being unreadable.
        let document = Document::open_bytes(&decoder, SYNTHETIC, &Limits::new())
            .expect("the synthetic document opens");
        let mut stream = document.decode(0).expect("view 0 decodes");
        let control: Vec<_> = stream.by_ref().collect();
        assert!(
            control.iter().all(Result::is_ok),
            "the unbounded control run should not refuse anything: {:?}",
            control.iter().find(|r| r.is_err())
        );
        assert!(stream.document_end().is_some());
        drop(stream);
        drop(document);

        let bounded = Limits::new().with_max_polyline_points(1);
        let document =
            Document::open_bytes(&decoder, SYNTHETIC, &bounded).expect("the open still succeeds");
        let stream = document.decode(0).expect("the decode still begins");
        let outcome: Vec<_> = stream.collect();
        let refusal = outcome
            .iter()
            .find_map(|item| item.as_ref().err())
            .expect("a polyline of four vertices under a bound of one has to be refused");
        assert_eq!(
            *refusal,
            Error::LimitExceeded,
            "a bound in viprs_acad_limits_v1 is LIMIT_EXCEEDED and nothing else"
        );
    }
}
