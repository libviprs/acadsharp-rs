//! The extents rule, on the public entry points, in a test that needs no
//! archive.
//!
//! There used to be two copies of this rule: `batch::ViewBegin::bounds` and
//! `View::extents`, byte-identical bodies, and only the first of them had a
//! test. Deleting the three finiteness lines from `View::extents` left the
//! whole suite green, which is the same as saying the safe API's copy of the
//! rule was not tested at all.
//!
//! There is one copy now, `batch::Bounds::from_extents`, and everything below
//! is a different way of reaching it: the rule itself, the wire layer's
//! reading of a `ViewBegin`, the safe API's reading of a `View`, and
//! `Extents::new`. All four run the same table, so removing the check from the
//! one implementation reds every one of them.

use acadsharp_rs::batch::Bounds;
use acadsharp_rs::{Extents, View};

/// Four ways of not being a rectangle, and why each one is here.
///
/// The first two are one `NaN` in a box that is otherwise perfectly ordinary,
/// in each of the two positions that matter: a minimum and a maximum. The
/// third is the all-`NaN` box, which a comparison-only check waves through
/// completely, because every one of `NaN > NaN` is false. The fourth is the
/// infinities, which order correctly (`-inf <= inf`) and are still not a box
/// anybody can compute a centre or a width from.
const NOT_A_BOX: [([f64; 4], &str); 4] = [
    ([f64::NAN, 0.0, 10.0, 10.0], "a NaN minimum"),
    ([0.0, 0.0, f64::NAN, 10.0], "a NaN maximum"),
    (
        [f64::NAN, f64::NAN, f64::NAN, f64::NAN],
        "all four NaN, which every comparison in the rule says is fine",
    ),
    (
        [f64::NEG_INFINITY, 0.0, f64::INFINITY, 10.0],
        "infinities, which do order the right way round",
    ),
];

/// A finite box the right way round, with four different numbers so a rule
/// that mixed two of them up would show it.
const A_REAL_BOX: [f64; 4] = [-100.25, -50.5, 100.75, 50.125];

fn view(extents: [f64; 4]) -> View {
    View::new(0, 0, extents, 0, "Model")
}

#[test]
fn a_non_finite_extent_is_not_a_box_on_any_entry_point() {
    for (extents, why) in NOT_A_BOX {
        assert_eq!(
            Bounds::from_extents(extents),
            None,
            "Bounds::from_extents took {extents:?} ({why})"
        );
        assert_eq!(
            view(extents).extents(),
            None,
            "View::extents took {extents:?} ({why})"
        );
        assert_eq!(
            Extents::new(extents),
            None,
            "Extents::new took {extents:?} ({why})"
        );

        // And the bytes still cross verbatim either way, which is the half of
        // this that was always right: a caller who wants to see what the file
        // actually said can.
        assert_eq!(
            view(extents).raw_extents().map(f64::to_bits),
            extents.map(f64::to_bits),
            "raw_extents stopped carrying the numbers it was given"
        );
    }
}

#[test]
fn a_finite_right_way_round_box_survives_all_of_it() {
    // The positive control. A rule that answered `None` to everything passes
    // the test above and breaks every real drawing, so this one has to be
    // beside it.
    let expected = Bounds::from_extents(A_REAL_BOX).expect("a real box is a box");
    assert_eq!(
        [
            expected.min_x,
            expected.min_y,
            expected.max_x,
            expected.max_y
        ],
        A_REAL_BOX
    );

    let from_view = view(A_REAL_BOX).extents().expect("and through a View");
    let from_new = Extents::new(A_REAL_BOX).expect("and through Extents::new");
    assert_eq!(from_view, from_new);
    assert_eq!(
        [
            from_view.min_x,
            from_view.min_y,
            from_view.max_x,
            from_view.max_y
        ],
        A_REAL_BOX,
        "four different numbers, so a rule that swapped two of them would say so here"
    );
}

#[test]
fn the_inverted_box_a_producer_writes_is_still_refused() {
    // The case the comparison was there for in the first place, and the one
    // real drawings actually hit: AutoCAD writes this pair into EXTMIN and
    // EXTMAX for a drawing with nothing in it.
    assert_eq!(Bounds::from_extents([1e20, 1e20, -1e20, -1e20]), None);
    assert_eq!(view([1e20, 1e20, -1e20, -1e20]).extents(), None);
    // And one axis the wrong way round on its own, because min_x <= max_x is
    // only half the box.
    assert_eq!(view([0.0, 10.0, 10.0, -10.0]).extents(), None);
    assert_eq!(view([10.0, 0.0, -10.0, 10.0]).extents(), None);
}

#[test]
fn the_two_names_for_a_rectangle_convert_both_ways() {
    // `batch::Bounds` is the wire layer's name and `Extents` is the safe API's,
    // and they stay two types on purpose: one is always present on a
    // `ViewBegin` and the other only exists when it is usable. The pair of
    // `From`s is what makes keeping them apart cost nothing.
    let bounds = Bounds::from_extents(A_REAL_BOX).expect("a real box");
    let extents = Extents::from(bounds);
    let back = Bounds::from(extents);
    assert_eq!(back, bounds, "a round trip through Extents changed the box");
}
