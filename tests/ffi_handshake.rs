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
