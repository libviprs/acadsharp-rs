//! The result codes, read out of the header and compared with the crate's.
//!
//! Both directions matter. A code the header has and the crate lacks is a
//! value the crate will meet and not recognise; a code the crate has and the
//! header lacks is a number somebody invented. The header is frozen and the
//! crate is not, so the header wins both arguments.

use std::collections::BTreeMap;

use acadsharp_rs::ffi;

mod common;

/// Every `#define` inside the header's "Result codes" section, by name.
///
/// I key on the section rather than on the `VIPRS_ACAD_` prefix because the
/// header has three other defines with that prefix: the include guard, the ABI
/// version and the wire version. None of them is a result code, and a test
/// that demanded a `pub const` for the include guard would be wrong in a way
/// that is annoying to argue with.
fn codes_from_header() -> BTreeMap<String, u64> {
    let text = common::header_text();
    let lines: Vec<&str> = text.lines().collect();

    let start = lines
        .iter()
        .position(|l| l.trim() == "* Result codes")
        .expect("the header has no `Result codes` banner, so this parser has no section to read");

    let mut out = BTreeMap::new();
    let mut seen_a_define = false;
    for line in &lines[start + 1..] {
        let trimmed = line.trim();
        // The next banner ends the section. Checking for it only after at
        // least one define means the banner that closes the section's own
        // comment block does not end it before it starts.
        if seen_a_define && trimmed.starts_with("/* ---") {
            break;
        }
        if !trimmed.starts_with("#define ") {
            continue;
        }
        seen_a_define = true;
        let defines = common::integer_defines(trimmed);
        for (name, value) in defines {
            assert!(
                out.insert(name.clone(), value).is_none(),
                "the header defines `{name}` twice in the result-codes section"
            );
        }
    }

    assert!(
        out.len() >= 8,
        "I parsed only {} result codes out of the header, which is fewer than the contract has ever had. \
         A parser that finds almost nothing and then agrees with the crate about it proves nothing.",
        out.len()
    );
    out
}

/// Every `pub const VIPRS_ACAD_*: u32` declared in `src/ffi.rs`, by name.
///
/// This reads the source text on purpose. `ffi::RESULT_CODES` below is the
/// table the comparison actually uses, and a constant somebody adds without a
/// table entry would otherwise slip past unnoticed.
fn constants_declared_in_ffi_source() -> Vec<String> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ffi.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));

    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub const ") else {
            continue;
        };
        let Some(name) = rest.split(':').next() else {
            continue;
        };
        let name = name.trim();
        if name.starts_with("VIPRS_ACAD_") {
            out.push(name.to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn the_crate_declares_exactly_the_codes_the_header_declares() {
    let header = codes_from_header();
    let crate_codes: BTreeMap<String, u64> = ffi::RESULT_CODES
        .iter()
        .map(|(name, value)| ((*name).to_string(), u64::from(*value)))
        .collect();

    let missing: Vec<&String> = header
        .keys()
        .filter(|k| !crate_codes.contains_key(*k))
        .collect();
    assert!(
        missing.is_empty(),
        "the header declares these result codes and `ffi.rs` does not: {missing:?}"
    );

    let invented: Vec<&String> = crate_codes
        .keys()
        .filter(|k| !header.contains_key(*k))
        .collect();
    assert!(
        invented.is_empty(),
        "`ffi.rs` declares these result codes and the header does not: {invented:?}"
    );

    for (name, expected) in &header {
        let actual = crate_codes[name];
        assert_eq!(
            *expected, actual,
            "`{name}` is {expected} in the header and {actual} in `ffi.rs`"
        );
    }
}

#[test]
fn the_table_lists_every_constant_the_source_declares() {
    let declared = constants_declared_in_ffi_source();
    assert!(
        !declared.is_empty(),
        "I found no `pub const VIPRS_ACAD_*` lines in `src/ffi.rs`, so this check is looking at the wrong thing"
    );

    let mut tabled: Vec<String> = ffi::RESULT_CODES
        .iter()
        .map(|(n, _)| (*n).to_string())
        .collect();
    tabled.sort();

    assert_eq!(
        declared, tabled,
        "`src/ffi.rs` declares one set of result-code constants and `RESULT_CODES` lists another. \
         The table is what every other check reads, so a constant missing from it is a constant nothing checks."
    );
}

#[test]
fn ok_is_zero_and_buffer_too_small_is_ten() {
    // Two anchors, both read from the header, so a parser that returned a
    // plausible-looking map of the wrong thing still fails here.
    let header = codes_from_header();
    assert_eq!(header.get("VIPRS_ACAD_OK"), Some(&0));
    assert_eq!(header.get("VIPRS_ACAD_BUFFER_TOO_SMALL"), Some(&10));
    assert_eq!(ffi::VIPRS_ACAD_OK, 0);
    assert_eq!(ffi::VIPRS_ACAD_BUFFER_TOO_SMALL, 10);
}
