//! The handshake comparison, in a test that needs no archive.
//!
//! This is the crate's only piece of policy and it used to be reachable only
//! through a `handshake` gated on `acadsharp_linked`. Three of the four CI
//! jobs never link, so three of the four never compiled it: clippy never
//! linted it, rustdoc never rendered it, and changing the `&&` in the
//! comparison to `||` broke nothing any of them could see. `abi::check` takes
//! the two numbers as arguments, so it runs everywhere, and the four rows
//! below are the whole truth table of a two-term `&&`.
//!
//! `tests/ffi_handshake.rs` still covers the other half, the one no table can:
//! that the library in `$ACADSHARP_NATIVE_DIR` reports the numbers the
//! vendored header declares.

use acadsharp_rs::abi::{self, EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION};

/// A version that is not the expected one, derived rather than typed so it
/// stays wrong after the header moves.
const DRIFTED_VERSION: u32 = EXPECTED_ABI_VERSION.wrapping_add(1);
/// The same idea for the fingerprint. One flipped bit is the realistic case:
/// a comment-only edit to the header changes every byte of the digest, and a
/// single differing bit is the smallest thing the check has to catch.
const DRIFTED_FINGERPRINT: u64 = EXPECTED_ABI_FINGERPRINT ^ 1;

#[test]
fn the_comparison_accepts_exactly_the_pair_the_header_declares() {
    // (version, fingerprint, should it pass, what this row is)
    let cases: [(u32, u64, bool, &str); 4] = [
        (
            EXPECTED_ABI_VERSION,
            EXPECTED_ABI_FINGERPRINT,
            true,
            "both match, which is the only row that may pass",
        ),
        (
            DRIFTED_VERSION,
            EXPECTED_ABI_FINGERPRINT,
            false,
            "the version drifted: the contract itself moved",
        ),
        (
            EXPECTED_ABI_VERSION,
            DRIFTED_FINGERPRINT,
            false,
            "the fingerprint drifted: same contract, different commit, and this is the row an \
             `||` instead of an `&&` would let through",
        ),
        (DRIFTED_VERSION, DRIFTED_FINGERPRINT, false, "both drifted"),
    ];

    for (version, fingerprint, should_pass, why) in cases {
        let outcome = abi::check(version, fingerprint);
        assert_eq!(
            outcome.is_ok(),
            should_pass,
            "{why}: check({version}, {fingerprint:#018x}) came back {outcome:?}"
        );

        let Err(mismatch) = outcome else {
            continue;
        };
        // A refusal carries all four numbers, not just the pair that differed,
        // because which pair matched is what says whether the contract moved
        // or only the build did.
        assert_eq!(mismatch.actual_abi_version, version, "{why}");
        assert_eq!(mismatch.actual_fingerprint, fingerprint, "{why}");
        assert_eq!(mismatch.expected_abi_version, EXPECTED_ABI_VERSION, "{why}");
        assert_eq!(
            mismatch.expected_fingerprint, EXPECTED_ABI_FINGERPRINT,
            "{why}"
        );
    }
}

#[test]
fn a_refusal_says_all_four_numbers_out_loud() {
    let mismatch = abi::check(DRIFTED_VERSION, DRIFTED_FINGERPRINT)
        .expect_err("a drifted pair has to be refused");
    let message = mismatch.to_string();

    for number in [
        format!("{DRIFTED_VERSION}"),
        format!("{EXPECTED_ABI_VERSION}"),
        format!("{DRIFTED_FINGERPRINT:#018x}"),
        format!("{EXPECTED_ABI_FINGERPRINT:#018x}"),
    ] {
        assert!(
            message.contains(&number),
            "a refusal that does not print {number} leaves the reader guessing which half drifted: {message}"
        );
    }
}
