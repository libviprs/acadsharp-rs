//! The error type, its mapping from native result codes, and what it says.
//!
//! None of this needs the archive. The mapping is a plain function over a
//! `u32` and a pair of drawing-version numbers, which is deliberate: the one
//! table every caller's control flow hangs off should be testable in the jobs
//! that never link a library, not only in the one that does.

use std::error::Error as _;

use acadsharp_rs::batch::{BatchError, Reason};
use acadsharp_rs::{Error, ffi};

/// The two numbers `Capabilities` reports for the pinned build, so the
/// `UnsupportedFormat` case carries something recognisable.
const DWG: (u32, u32) = (1014, 1032);

#[test]
fn every_code_the_header_declares_maps_to_something_better_than_native() {
    // The non-tautological half: the list comes from `ffi::RESULT_CODES`,
    // which `tests/ffi_codes.rs` already pins against the header's own
    // `#define`s in both directions. So a code the header gains and this
    // mapping has not is a failing test here rather than a number a caller
    // meets and cannot name.
    for (name, code) in ffi::RESULT_CODES {
        let mapped = Error::from_native_code(*code, DWG.0, DWG.1);
        if *code == ffi::VIPRS_ACAD_OK {
            assert_eq!(mapped, None, "{name} is success and is not an error");
            continue;
        }
        let mapped = mapped.unwrap_or_else(|| panic!("{name} should map to an error"));
        assert!(
            !matches!(mapped, Error::Native(_)),
            "{name} ({code}) fell through to Error::Native, which is the arm for codes the \
             header does not declare"
        );
    }
}

#[test]
fn the_code_table_is_exactly_this() {
    // A predicate per row rather than a value, because `UnsupportedFormat` is
    // `#[non_exhaustive]` and a test outside this crate cannot build one. The
    // row for it still names both of its fields and checks both numbers, so it
    // is the same assertion wearing a different shape.
    /// The code, whether a mapped error is the right one, and its name.
    type Row = (u32, fn(&Error) -> bool, &'static str);

    let cases: [Row; 8] = [
        (
            1,
            |e| matches!(e, Error::InvalidArgument),
            "InvalidArgument",
        ),
        (
            2,
            |e| {
                matches!(
                    e,
                    Error::UnsupportedFormat {
                        dwg_version_min,
                        dwg_version_max,
                        ..
                    } if (*dwg_version_min, *dwg_version_max) == DWG
                )
            },
            "UnsupportedFormat carrying the drawing range",
        ),
        (3, |e| matches!(e, Error::CorruptInput), "CorruptInput"),
        (
            4,
            |e| matches!(e, Error::UnsupportedEntity),
            "UnsupportedEntity",
        ),
        (5, |e| matches!(e, Error::OutOfMemory), "OutOfMemory"),
        (6, |e| matches!(e, Error::Cancelled), "Cancelled"),
        (8, |e| matches!(e, Error::AbiMismatch), "AbiMismatch"),
        (9, |e| matches!(e, Error::LimitExceeded), "LimitExceeded"),
    ];
    for (code, is_it, name) in cases {
        let mapped = Error::from_native_code(code, DWG.0, DWG.1)
            .unwrap_or_else(|| panic!("code {code} should map to {name} and mapped to success"));
        assert!(
            is_it(&mapped),
            "code {code} should map to {name}, got {mapped:?}"
        );
    }
}

#[test]
fn an_unsupported_format_carries_the_range_a_caller_is_told_to_look_at() {
    // ABI.md's whole sentence for code 2 is "the input is a format, or a
    // version of one, this build does not read. Check dwg_version_min and
    // dwg_version_max." An error that does not carry them makes the caller go
    // and ask a second time.
    let Some(Error::UnsupportedFormat {
        dwg_version_min,
        dwg_version_max,
        ..
    }) = Error::from_native_code(2, 1014, 1032)
    else {
        panic!("code 2 is UnsupportedFormat");
    };
    assert_eq!((dwg_version_min, dwg_version_max), (1014, 1032));
    assert!(
        format!("{}", Error::from_native_code(2, 1014, 1032).unwrap()).contains("1032"),
        "the range belongs in the message too, because that is what gets logged"
    );
}

#[test]
fn the_two_internal_codes_are_internal_and_not_native() {
    // 7 is the library's own bug channel and 10 is about a buffer this crate
    // owns, which a caller must never be handed. Both are `Internal`, and the
    // string beside them says which.
    for code in [7u32, 10] {
        let mapped = Error::from_native_code(code, DWG.0, DWG.1).unwrap();
        assert!(
            matches!(mapped, Error::Internal { .. }),
            "code {code} should be Internal, got {mapped:?}"
        );
    }
}

#[test]
fn an_undeclared_code_becomes_native_rather_than_a_panic_or_a_silent_ok() {
    for code in [11u32, 42, 1000, u32::MAX] {
        assert_eq!(
            Error::from_native_code(code, DWG.0, DWG.1),
            Some(Error::Native(code)),
            "a code nothing declares has to survive as a number"
        );
    }
}

#[test]
fn cancelled_is_spelled_with_two_ls_and_the_message_says_so() {
    let e = Error::from_native_code(6, DWG.0, DWG.1).unwrap();
    assert_eq!(e, Error::Cancelled);
    let text = format!("{e}");
    assert!(
        text.contains("cancel"),
        "the message should say what happened, and it says {text:?}"
    );
}

#[test]
fn error_is_a_std_error_and_hands_back_the_inner_one_where_there_is_one() {
    let inner = BatchError::CorruptInput {
        offset: 12,
        reason: Reason::BadMagic,
    };
    let wrapped = Error::Batch(inner);
    let source = wrapped
        .source()
        .expect("a wrapped batch error has a source");
    assert_eq!(
        format!("{source}"),
        format!("{inner}"),
        "source() should be the BatchError itself, not a copy of the outer message"
    );
    assert!(
        source.downcast_ref::<BatchError>().is_some(),
        "a caller should be able to downcast back to the batch error"
    );

    // And the plain variants have no source to hand back.
    assert!(Error::Cancelled.source().is_none());
}

#[test]
fn a_path_that_is_not_utf8_has_its_own_variant_and_no_io_error_anywhere() {
    // The issue's sketch had `Io(std::io::Error)`. Opening a path never reads
    // the file in Rust, so there is no `io::Error` to carry: the failure this
    // crate actually has is a path whose bytes are not UTF-8, which the
    // boundary cannot take.
    let e = Error::PathNotUtf8;
    assert!(format!("{e}").to_lowercase().contains("utf-8"));
    assert!(e.source().is_none());
}

#[test]
fn is_unlinked_is_askable_because_a_caller_cannot_write_the_cfg() {
    assert!(Error::Unlinked.is_unlinked());
    assert!(!Error::Cancelled.is_unlinked());
}
