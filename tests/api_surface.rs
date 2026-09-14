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
//!
//! The `unsafe` half of that used to live here too, as a grep over a list of
//! eight filenames somebody typed. It missed `abi.rs`, which was not on the
//! list and did contain `unsafe`, and it matched `"unsafe "` with a trailing
//! space, so `unsafe{` went straight past it. `#![deny(unsafe_code)]` at the
//! crate root with `#[allow(unsafe_code)]` on the two modules that need one
//! does that job properly: it is a compile error, it cannot miss a spelling,
//! and a new module is covered the moment it exists. What is left here is the
//! part no lint covers, and the list it runs over is read from the directory.

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

/// The two modules that are allowed to name a pointer and an `ffi` type: one
/// is the transcription of the C header and the other is the seam that calls
/// it. Everything else in `src/` is the safe API and is scanned.
///
/// An explicit list of exemptions rather than an explicit list of members,
/// which is the whole point: a module added tomorrow is covered by default,
/// and exempting one is a diff somebody has to read.
const EXEMPT: [&str; 2] = ["ffi.rs", "sys.rs"];

fn modules_in_src() -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
            .expect("src/ is readable")
            .map(|entry| entry.expect("a directory entry").file_name())
            .filter_map(|name| name.into_string().ok())
            .filter(|name| name.ends_with(".rs"))
            .collect();
    names.sort();
    names
}

/// Every module that makes up the safe API, read from the directory.
fn api_modules() -> Vec<String> {
    modules_in_src()
        .into_iter()
        .filter(|name| !EXEMPT.contains(&name.as_str()))
        .collect()
}

#[test]
fn the_scanned_list_is_the_directory_minus_a_list_of_exemptions() {
    // Two ways this goes quietly wrong. A `read_dir` that came back empty
    // would make every scan below pass by scanning nothing, and an exemption
    // naming a file that no longer exists would sit there widening nothing
    // while somebody assumes it covers something.
    let all = modules_in_src();
    assert!(
        all.len() >= 10,
        "src/ came back with {} modules, which is not this crate: {all:?}",
        all.len()
    );
    for exempt in EXEMPT {
        assert!(
            all.contains(&exempt.to_string()),
            "EXEMPT names src/{exempt}, which is not in src/ any more, so it exempts nothing \
             and the module it used to name is either gone or scanned under another name"
        );
    }
    let scanned = api_modules();
    assert_eq!(scanned.len(), all.len() - EXEMPT.len());
    for name in ["abi.rs", "batch.rs", "item.rs", "stream.rs", "document.rs"] {
        assert!(
            scanned.contains(&name.to_string()),
            "src/{name} is not being scanned"
        );
    }
}

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
    for module in api_modules() {
        let module = module.as_str();
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
fn the_unsafe_exemption_is_two_lines_in_the_crate_root_and_nothing_else() {
    // `#![deny(unsafe_code)]` is what actually stops an `unsafe` block in the
    // safe API, and the compiler cannot be talked out of it by a spelling. What
    // it can be talked out of is another `#[allow(unsafe_code)]`, so this
    // counts them: two, both in `lib.rs`, both on a module declaration naming
    // something in `EXEMPT`.
    let root = read("lib.rs");
    assert!(
        root.contains("#![deny(unsafe_code)]"),
        "the crate root stopped denying unsafe_code, which is the whole instrument"
    );

    let mut allowed = Vec::new();
    let mut lines = root.lines().enumerate().peekable();
    while let Some((number, line)) = lines.next() {
        if line.trim().starts_with("//") || !line.contains("allow(unsafe_code)") {
            continue;
        }
        let (_, next) = *lines.peek().unwrap_or_else(|| {
            panic!(
                "src/lib.rs:{} allows unsafe_code and declares nothing",
                number + 1
            )
        });
        allowed.push(next.trim().to_string());
    }
    assert_eq!(
        allowed,
        vec!["pub mod ffi;".to_string(), "mod sys;".to_string()],
        "the unsafe exemption moved. It is two module declarations in src/lib.rs and every \
         other module in this crate is denied an `unsafe` block by the compiler"
    );

    for module in api_modules() {
        assert!(
            !EXEMPT.contains(&module.as_str()),
            "src/{module} is scanned and exempt at the same time"
        );
    }
}

#[test]
fn no_unsafe_impl_anywhere_in_the_crate() {
    // Absence of `Send` and `Sync` comes free from holding a raw pointer.
    // Reaching for `unsafe impl` to put one back is the failure mode this
    // guards, and it would silently satisfy every other test in the suite.
    for module in modules_in_src() {
        let module = module.as_str();
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
fn the_raw_ffi_module_is_not_part_of_the_documented_surface() {
    // It has to stay `pub`: the tests in this directory compile against it
    // from outside the crate, which is the whole point of testing a
    // transcription. It must not stay documented. Measured from a consumer
    // crate, `*mut acadsharp_rs::ffi::viprs_acad_handle` and the
    // `unsafe extern "C"` function values are reachable, which contradicts the
    // first paragraph of `src/lib.rs` and freezes eleven C signatures into a
    // 0.1.0 semver promise. `docs/UPGRADING.md` exists because the header does
    // move.
    let root = read("lib.rs");
    let mut lines = root.lines().enumerate();
    let declaration = lines
        .find(|(_, line)| line.trim() == "pub mod ffi;")
        .map(|(number, _)| number)
        .expect("src/lib.rs declares `pub mod ffi;`");
    let before: Vec<&str> = root.lines().take(declaration).collect();
    let attributes: Vec<&str> = before
        .iter()
        .rev()
        .take_while(|line| line.trim().starts_with('#'))
        .copied()
        .collect();
    assert!(
        attributes.iter().any(|line| line.contains("doc(hidden)")),
        "`pub mod ffi;` in src/lib.rs is not `#[doc(hidden)]`, so the raw pointers and the \
         eleven `unsafe extern \"C\"` signatures are published API: {attributes:?}"
    );

    // And the two that are documented on purpose are still documented.
    for documented in ["pub mod batch;", "pub mod abi;"] {
        let declaration = root
            .lines()
            .position(|line| line.trim() == documented)
            .unwrap_or_else(|| panic!("src/lib.rs declares `{documented}`"));
        let hidden = root
            .lines()
            .take(declaration)
            .last()
            .is_some_and(|line| line.contains("doc(hidden)"));
        assert!(
            !hidden,
            "`{documented}` got hidden. `batch` is safe, standalone and useful on its own, and \
             `abi` carries the three constants and `check`"
        );
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
