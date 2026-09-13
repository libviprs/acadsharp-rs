//! The handshake, against the real library.
//!
//! There is no stub here and there is not going to be one. A stub proves that
//! the comparison in `handshake` works; it cannot prove that the library
//! sitting in `$ACADSHARP_NATIVE_DIR` agrees with the header this crate
//! vendored, which is the only question worth asking. `tests/abi_constants.rs`
//! covers the other half by re-deriving the constants from the header, so
//! between them both sides of the comparison are pinned to something real.
//!
//! Everything here needs the archive, so the whole file is gated on
//! `acadsharp_linked` and `tests/native_lane_is_live.rs` makes sure that gate
//! cannot quietly stay shut in a job that was supposed to open it.
#![cfg(acadsharp_linked)]

use std::ptr;

use acadsharp_rs::ffi;
use acadsharp_rs::{EXPECTED_ABI_FINGERPRINT, EXPECTED_ABI_VERSION, EXPECTED_WIRE_VERSION};

#[test]
fn the_library_reports_the_abi_version_the_vendored_header_declares() {
    // SAFETY: `viprs_acad_abi_version` takes no arguments, returns a plain
    // `uint32_t` and the header says it never fails, so the only precondition
    // is that the symbol is linked. `cfg(acadsharp_linked)` is exactly that.
    let actual = unsafe { ffi::viprs_acad_abi_version() };
    assert_eq!(
        actual, EXPECTED_ABI_VERSION,
        "the library reports ABI version {actual} and the vendored header declares {EXPECTED_ABI_VERSION}"
    );
}

#[test]
fn the_library_reports_the_fingerprint_of_the_vendored_header() {
    // SAFETY: same as above. No arguments, a `uint64_t` back, never fails.
    let actual = unsafe { ffi::viprs_acad_abi_fingerprint() };
    assert_eq!(
        actual, EXPECTED_ABI_FINGERPRINT,
        "the library reports fingerprint {actual:#018x} and the vendored header hashes to {EXPECTED_ABI_FINGERPRINT:#018x}. \
         That means the header and the library came from different commits."
    );
}

#[test]
fn the_handshake_succeeds_against_the_pinned_archive() {
    // SAFETY: `handshake` only makes the two calls above, both of which are
    // argument-free and infallible, so linking is its whole precondition.
    let result = unsafe { ffi::handshake() };
    assert_eq!(result, Ok(()), "the handshake refused the pinned archive");
}

#[test]
fn capabilities_answer_what_the_contract_says_they_answer() {
    let mut caps = ffi::viprs_acad_capabilities_v1 {
        struct_size: u32::try_from(size_of::<ffi::viprs_acad_capabilities_v1>()).unwrap(),
        struct_version: 1,
        ..Default::default()
    };
    let mut required: u64 = 0;

    // SAFETY: `caps` is a live, fully initialised struct of the type the callee
    // expects, with `struct_size` and `struct_version` set as the header
    // requires. The buffer is a real null with a capacity of zero, which is the
    // documented way to ask for the length rather than a pointer I invented,
    // and `required` is a live `u64` the callee may write.
    let code = unsafe {
        ffi::viprs_acad_get_capabilities_v1(&mut caps, ptr::null_mut(), 0, &mut required)
    };

    assert_eq!(
        code,
        ffi::VIPRS_ACAD_OK,
        "the sizing call is documented to return OK, not a buffer code, and it returned {code}"
    );
    assert_eq!(
        required, 5,
        "the pinned build reports ACadSharp \"3.7.1\", which is five bytes"
    );

    assert_eq!(caps.abi_version, EXPECTED_ABI_VERSION);
    assert_eq!(caps.wire_version, EXPECTED_WIRE_VERSION);
    assert_eq!(caps.dwg_version_min, 1014);
    assert_eq!(caps.dwg_version_max, 1032);
    assert_eq!(caps.supports_block_expansion, 1);
    assert_eq!(caps.supports_warnings, 1);
}

/// The synthetic document every native test in this crate opens.
///
/// `VIPRSSYN` plus a view count and a primitive count, both `u32`
/// little-endian, is the library's own contractual probe input. It needs no
/// drawing file and no fixture, and the contract pins what comes out of it.
fn synthetic_document() -> Vec<u8> {
    let mut bytes = b"VIPRSSYN".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes
}

#[test]
fn every_default_struct_is_one_the_library_accepts() {
    // The whole point of `Default` on these three. Every out-struct on this
    // boundary carries a `struct_size` and a `struct_version` the caller sets,
    // and a callee handed a `struct_size` it does not recognise answers
    // INVALID_ARGUMENT rather than reading past what was allocated. So an
    // all-zero default is a struct the library refuses by design, and a
    // default that has to be repaired by hand at every call site is a default
    // that is wrong.
    let mut caps = ffi::viprs_acad_capabilities_v1::default();
    let mut required: u64 = 0;
    // SAFETY: `caps` is a live, fully initialised struct of the type the
    // callee expects. The buffer is a real null with a capacity of zero, the
    // documented way to ask for the length, and `required` is a live `u64` the
    // callee may write.
    let code = unsafe {
        ffi::viprs_acad_get_capabilities_v1(&mut caps, ptr::null_mut(), 0, &mut required)
    };
    assert_eq!(
        code,
        ffi::VIPRS_ACAD_OK,
        "the library refused `viprs_acad_capabilities_v1::default()` with code {code}, so the \
         default this crate hands out is one no caller can use without repairing it first"
    );
    assert_eq!(caps.abi_version, EXPECTED_ABI_VERSION);

    let limits = ffi::viprs_acad_limits_v1::default();
    let document = synthetic_document();
    let mut handle = ptr::null_mut();
    // SAFETY: `document` is a live buffer I own for the whole call, `limits` is
    // a live struct of the type the callee expects, and `handle` is a live
    // out-pointer. The library is documented to own `data` for the duration of
    // this call only.
    let code = unsafe {
        ffi::viprs_acad_open_memory(
            document.as_ptr(),
            document.len() as u64,
            &limits,
            &mut handle,
        )
    };
    assert_eq!(
        code,
        ffi::VIPRS_ACAD_OK,
        "the library refused `viprs_acad_limits_v1::default()` with code {code}, and an all-zero \
         limits struct is supposed to mean `every bound is yours to pick`"
    );
    assert!(!handle.is_null());

    let mut info = ffi::viprs_acad_view_info_v1::default();
    let mut name_required: u64 = 0;
    // SAFETY: `handle` came back non-null from the open call above and has not
    // been closed, `info` is a live struct of the right type, and the name
    // buffer is a real null with a capacity of zero.
    let code = unsafe {
        ffi::viprs_acad_get_view_info_v1(
            handle,
            0,
            &mut info,
            ptr::null_mut(),
            0,
            &mut name_required,
        )
    };
    // SAFETY: `handle` is a handle this library issued and I have not closed
    // it yet. Closing it here rather than after the assertion, so a failing
    // assertion does not leak the document.
    unsafe { ffi::viprs_acad_close(handle) };

    assert_eq!(
        code,
        ffi::VIPRS_ACAD_OK,
        "the library refused `viprs_acad_view_info_v1::default()` with code {code}"
    );
    assert_eq!(info.index, 0, "the view info echoes the index back");
}
