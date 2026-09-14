//! The things that must not compile.
//!
//! Two claims live here. One: `Decoder`, `Document` and `PrimitiveStream` are
//! neither `Send` nor `Sync`, which falls out of holding a raw pointer and is
//! deliberate rather than accidental. One decode handle is single threaded and
//! calls on it must not overlap, so a handle that crossed a thread boundary
//! would be a data race the type system had waved through. ABI.md does allow
//! two decode handles on two threads, so this is liftable later behind a type
//! that owns the pairing; it is not liftable by an `unsafe impl`.
//!
//! Two: a `Document` cannot be dropped while a stream borrows it.
//! `viprs_acad_close` releases every decode handle still open on the document,
//! so the ordering is not a style preference, and `PrimitiveStream<'doc>`
//! borrowing the document is what hands the whole question to borrowck.
//!
//! The expected output in `tests/compile_fail/*.stderr` is rustc's, so it can
//! go stale when the toolchain moves. `TRYBUILD=overwrite cargo test` rewrites
//! it. `tests/api_surface.rs` asserts the same two `Send`/`Sync` facts through
//! a trait ambiguity that no toolchain update can invalidate, so a stale file
//! here is a chore rather than a hole.

#[test]
fn the_handles_stay_on_one_thread_and_a_document_outlives_its_stream() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
