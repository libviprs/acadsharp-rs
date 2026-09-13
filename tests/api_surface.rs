//! What the public surface is allowed to contain, checked rather than
//! promised.
//!
//! Two instruments. The trait assertions below are compile-time and are
//! toolchain proof: each one compiles exactly while the type does *not*
//! implement the trait, so an `unsafe impl Send` added anywhere turns this
//! file red without a single expected-output file to regenerate.
//! `tests/api_compile_fail.rs` says the same thing through `trybuild`, which
//! also pins the message a caller sees. Two instruments, because the
//! `trybuild` one is the one that goes stale on a new rustc.
//!
//! The source scans are the crude half and they are still worth having: no
//! doc-scrape exists that runs on stable, and "no raw pointer in the public
//! API" is otherwise a claim nobody can fail.

use std::path::{Path, PathBuf};

use acadsharp_rs::{
    CancelToken, Capabilities, Decoder, Document, Error, Extents, Item, Limits, Primitive,
    PrimitiveStream, View, ViewKind, Warning, WarningCode,
};

// ---------------------------------------------------------------------------
// Compile-time trait assertions
// ---------------------------------------------------------------------------

/// Compiles exactly while `$t` does **not** implement `$trait_`.
///
/// Every type implements `AmbiguousIfImpl<()>`, and one that implements the
/// trait implements `AmbiguousIfImpl<u8>` as well, so naming the method on it
/// becomes ambiguous and the inference variable cannot be resolved.
macro_rules! assert_not_impl {
    ($t:ty, $trait_:path) => {
        const _: fn() = || {
            #[allow(dead_code)]
            trait AmbiguousIfImpl<A> {
                fn maybe() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            impl<T: ?Sized + $trait_> AmbiguousIfImpl<u8> for T {}
            let _ = <$t as AmbiguousIfImpl<_>>::maybe;
        };
    };
}

assert_not_impl!(Decoder, Send);
assert_not_impl!(Decoder, Sync);
assert_not_impl!(Document, Send);
assert_not_impl!(Document, Sync);
assert_not_impl!(PrimitiveStream<'static>, Send);
assert_not_impl!(PrimitiveStream<'static>, Sync);

const fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn the_data_types_cross_threads_even_though_the_handles_do_not() {
    // The handles are single threaded because one decode handle is single
    // threaded and calls on it must not overlap. What comes *out* of them is
    // plain owned data and has no such problem, and a caller who cannot send
    // a decoded primitive to a worker gains nothing from the restriction.
    assert_send_sync::<Item>();
    assert_send_sync::<Primitive>();
    assert_send_sync::<Warning>();
    assert_send_sync::<WarningCode>();
    assert_send_sync::<View>();
    assert_send_sync::<ViewKind>();
    assert_send_sync::<Extents>();
    assert_send_sync::<Limits>();
    assert_send_sync::<Capabilities>();
    assert_send_sync::<Error>();
    // And the cancel flag, which is the one word two threads touch at once.
    assert_send_sync::<CancelToken>();
}

// ---------------------------------------------------------------------------
// Source scans
// ---------------------------------------------------------------------------

fn src(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)
}

/// Every module that makes up the safe API. `ffi` and `sys` are deliberately
/// absent: one is the transcription and the other is the only place `unsafe`
/// is allowed to live.
const API_MODULES: [&str; 8] = [
    "lib.rs",
    "error.rs",
    "limits.rs",
    "capabilities.rs",
    "cancel.rs",
    "document.rs",
    "item.rs",
    "stream.rs",
];

fn read(name: &str) -> String {
    let path = src(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Lines that declare something `pub`, with `pub(crate)` and friends dropped,
/// and with doc comments dropped because prose is allowed to name anything.
fn public_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(n, line)| (n + 1, line.trim()))
        .filter(|(_, line)| !line.starts_with("//"))
        .filter(|(_, line)| line.starts_with("pub ") || line.contains(" pub "))
        .filter(|(_, line)| !line.contains("pub(crate)") && !line.contains("pub(super)"))
        .collect()
}

#[test]
fn no_raw_pointer_and_no_ffi_type_is_reachable_from_the_public_api() {
    for module in API_MODULES {
        let text = read(module);
        for (number, line) in public_lines(&text) {
            for forbidden in ["*mut", "*const", "ffi::", "viprs_acad_"] {
                assert!(
                    !line.contains(forbidden),
                    "src/{module}:{number} puts `{forbidden}` on a public declaration, and the \
                     whole point of this crate is that neither .NET nor the C boundary reaches \
                     a caller: {line}"
                );
            }
        }
    }
}

#[test]
fn unsafe_lives_in_ffi_and_sys_and_nowhere_else() {
    for module in API_MODULES {
        let text = read(module);
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with("//") || line.starts_with("#!") {
                continue;
            }
            assert!(
                !line.contains("unsafe "),
                "src/{module}:{} reaches for unsafe, and the safe API is not where that \
                 belongs: {line}",
                number + 1
            );
        }
    }
}

#[test]
fn no_unsafe_impl_anywhere_in_the_crate() {
    // Absence of `Send` and `Sync` comes free from holding a raw pointer.
    // Reaching for `unsafe impl` to put one back is the failure mode this
    // guards, and it would silently satisfy every other test in the suite.
    for module in API_MODULES
        .iter()
        .chain(["ffi.rs", "sys.rs", "abi.rs", "batch.rs"].iter())
    {
        let text = read(module);
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            // Prose is allowed to name the thing it is refusing, and this
            // crate's documentation names it several times.
            if line.starts_with("//") {
                continue;
            }
            assert!(
                !line.contains("unsafe impl"),
                "src/{module}:{} carries an `unsafe impl`: {line}",
                number + 1
            );
        }
    }
}

#[test]
fn the_limits_module_carries_no_copy_of_a_documented_default() {
    // A hand-carried 65536 keeps confidently reporting the old bound after the
    // native default moves, and it reports it as though the library agreed.
    // Every field is `Option`, and `None` means the library picks.
    let text = read("limits.rs");
    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        for number_literal in [
            "65536",
            "65_536",
            "536870912",
            "536_870_912",
            "20000000",
            "20_000_000",
            "1000000",
            "1_000_000",
            "4294967296",
            "4_294_967_296",
        ] {
            assert!(
                !trimmed.contains(number_literal),
                "src/limits.rs:{} writes down {number_literal}, which is a Rust side copy of a \
                 bound the library owns: {trimmed}",
                number + 1
            );
        }
    }
}

#[test]
fn the_crate_declares_no_runtime_dependency() {
    // The safe API is where a convenience crate would have crept in. `trybuild`
    // is a dev-dependency and never reaches a consumer's binary.
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the manifest");
    let after = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("a [dependencies] section");
    let section = after.split("\n[").next().unwrap_or("");
    for line in section.lines() {
        let line = line.trim();
        assert!(
            line.is_empty() || line.starts_with('#'),
            "a runtime dependency appeared: {line}"
        );
    }
}
