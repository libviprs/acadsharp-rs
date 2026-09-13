//! The three generated constants, and the hash they are generated with.
//!
//! `build.rs` derives `EXPECTED_ABI_VERSION`, `EXPECTED_WIRE_VERSION` and
//! `EXPECTED_ABI_FINGERPRINT` from the bytes of `native/viprs_acadsharp.h`.
//! Nobody types those numbers into Rust source, so nothing in the crate can
//! disagree with the header by hand. This test re-derives all three a second
//! time, here, and compares.
//!
//! It also compiles `build/sha256.rs` into a real test target and runs the NIST
//! vectors through it. Without that the hash in the build script would be
//! checked by nothing at all: a build script's `#[cfg(test)]` module is never
//! compiled, and a test that never runs is the same colour as one that passed.

use acadsharp_rs::{EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};

#[allow(dead_code)]
#[path = "../build/sha256.rs"]
mod sha256;

mod common;

/// FIPS 180-4 appendix B.1: the digest of the empty message.
const EMPTY_VECTOR: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// FIPS 180-4 appendix B.1: the digest of "abc", one block.
const ABC_VECTOR: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
/// FIPS 180-4 appendix B.2: 56 bytes, so the padding spills into a second
/// block. This is the case a one-block implementation gets wrong.
const TWO_BLOCK_MESSAGE: &str = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
const TWO_BLOCK_VECTOR: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";

/// The digest the repository committed for the vendored header, read out of
/// `native/viprs_acadsharp.h.sha256`.
///
/// The file is written in `sha256sum` format so `sha256sum -c` works on it
/// unchanged, so the digest is the first whitespace-separated token.
fn committed_digest() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native/viprs_acadsharp.h.sha256");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));
    text.split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("{} is empty", path.display()))
        .to_string()
}

#[test]
fn sha256_matches_the_nist_vectors() {
    assert_eq!(sha256::hex(&sha256::sha256(b"")), EMPTY_VECTOR);
    assert_eq!(sha256::hex(&sha256::sha256(b"abc")), ABC_VECTOR);
    assert_eq!(
        sha256::hex(&sha256::sha256(TWO_BLOCK_MESSAGE.as_bytes())),
        TWO_BLOCK_VECTOR
    );
}

#[test]
fn sha256_matches_the_committed_digest_of_the_vendored_header() {
    let bytes = std::fs::read(common::header_path()).expect("the vendored header is missing");
    assert_eq!(
        sha256::hex(&sha256::sha256(&bytes)),
        committed_digest(),
        "the vendored header no longer hashes to the digest committed beside it"
    );
}

#[test]
fn the_fingerprint_is_the_first_eight_bytes_of_that_digest() {
    let bytes = std::fs::read(common::header_path()).expect("the vendored header is missing");
    let digest = sha256::sha256(&bytes);
    let expected = u64::from_be_bytes(digest[..8].try_into().unwrap());
    assert_eq!(
        EXPECTED_ABI_FINGERPRINT, expected,
        "`EXPECTED_ABI_FINGERPRINT` is not the leading eight bytes of the vendored header's digest, read big-endian"
    );
}

#[test]
fn the_versions_come_from_the_header() {
    let text = common::header_text();
    let name = common::header_name();
    let lookup = |define: &str| -> u64 { common::header::integer_define(&text, define, &name) };

    assert_eq!(
        u64::from(EXPECTED_ABI_VERSION),
        lookup("VIPRS_ACAD_ABI_VERSION"),
        "`EXPECTED_ABI_VERSION` and the header's `VIPRS_ACAD_ABI_VERSION` disagree"
    );
    assert_eq!(
        u64::from(EXPECTED_WIRE_VERSION),
        lookup("VIPRS_ACAD_WIRE_VERSION"),
        "`EXPECTED_WIRE_VERSION` and the header's `VIPRS_ACAD_WIRE_VERSION` disagree"
    );
}

/// The exact shape the review proved past the build script: a `#define` for a
/// name the header also defines properly, sitting inside a block comment.
///
/// A C compiler reads the second one and this crate has to read the same one,
/// or every constant it generates describes a header nobody compiled.
const A_DEFINE_INSIDE_A_COMMENT: &str = "\
/* an example from an older revision:
#define VIPRS_ACAD_ABI_VERSION 1u
*/
#define VIPRS_ACAD_ABI_VERSION 2u
";

/// Two live definitions of one name. C would take the second and warn about
/// the redefinition, and I would rather not pick at all.
const A_NAME_DEFINED_TWICE: &str = "\
#define VIPRS_ACAD_ABI_VERSION 2u
#define VIPRS_ACAD_ABI_VERSION 3u
";

#[test]
fn a_define_inside_a_block_comment_does_not_win() {
    let value = common::header::integer_define(
        A_DEFINE_INSIDE_A_COMMENT,
        "VIPRS_ACAD_ABI_VERSION",
        "the snippet in this test",
    );
    assert_eq!(
        value, 2,
        "the header parser took the `#define` inside the block comment, so every constant it \
         generates describes a header no C compiler would agree with"
    );
}

#[test]
fn a_name_the_header_defines_twice_is_refused() {
    let message = common::refusal("a name defined twice", || {
        let _ = common::header::integer_define(
            A_NAME_DEFINED_TWICE,
            "VIPRS_ACAD_ABI_VERSION",
            "the snippet in this test",
        );
    });
    assert!(
        message.contains("VIPRS_ACAD_ABI_VERSION") && message.contains("2 times"),
        "the refusal has to say which name is defined twice and how often, and it said: {message}"
    );
}
