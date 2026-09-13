//! Three jobs, and they are deliberately small.
//!
//! The first is to turn the bytes of `native/viprs_acadsharp.h` into the three
//! constants the crate compares the library against: the ABI version, the wire
//! version and the fingerprint. All three are read out of the header rather
//! than typed into Rust source anywhere. A number somebody typed drifts from
//! the file it describes the first time that file changes, and it drifts in
//! the one direction that does damage, by carrying on reporting agreement.
//!
//! The second is `COMPAT.toml`, the compatibility declaration, which is read
//! and then checked against everything it declares: the two versions and the
//! digest against the header, and the two artifact fields against the
//! archive's manifest. A value in there that disagrees stops the build. Why
//! the declaration is checked rather than believed is in `build/compat.rs`.
//!
//! The third is to find the native archive, if there is one, and emit the
//! link lines for it. That half is deliberately the thin version: shared
//! linking only, one hand-rolled reader for one field of `LINKINFO.json`, no
//! `serde`, no features, no static recipe. Issue #3 replaces exactly this half
//! with the real resolver (both link kinds, the `+whole-archive` ordering, the
//! full manifest under `serde`, and the `link-static` / `link-shared` feature
//! pair). Everything above the `Archive resolution` banner is this issue's and
//! stays.
//!
//! The crate has to build with no archive at all, because `Check & Lint`,
//! `MSRV` and `Docs` never link one. So a missing archive is a warning and an
//! absent `cfg`, never an error.

// ---------------------------------------------------------------------------
// Issue #6 (H3.4) owns `build/compat.rs` and every region below marked with
// its number. `#[allow(dead_code)]` because the README rendering half of that
// module is `tests/compat.rs`' to use and this script never calls it.
// ---------------------------------------------------------------------------
#[allow(dead_code)]
#[path = "build/compat.rs"]
mod compat;
#[path = "build/header.rs"]
mod header;
#[path = "build/linkinfo.rs"]
mod linkinfo;
#[path = "build/sha256.rs"]
mod sha256;

use std::path::{Path, PathBuf};

/// The vendored header, relative to the manifest directory.
const HEADER: &str = "native/viprs_acadsharp.h";
/// The digest committed beside it, in `sha256sum` format.
const HEADER_DIGEST: &str = "native/viprs_acadsharp.h.sha256";
// --- issue #6 (H3.4) -------------------------------------------------------
/// The compatibility declaration, relative to the manifest directory.
const COMPAT: &str = compat::COMPAT_FILE;
// --- end issue #6 ----------------------------------------------------------

fn main() {
    // Emitting any rerun-if-changed turns off cargo's default "rerun when
    // anything in the package changed", so every input this script reads has
    // to be listed, this file and the hash it pulls in included.
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build/header.rs");
    println!("cargo::rerun-if-changed=build/linkinfo.rs");
    println!("cargo::rerun-if-changed=build/sha256.rs");
    println!("cargo::rerun-if-changed={HEADER}");
    println!("cargo::rerun-if-changed={HEADER_DIGEST}");
    // --- issue #6 (H3.4) ---------------------------------------------------
    println!("cargo::rerun-if-changed=build/compat.rs");
    println!("cargo::rerun-if-changed={COMPAT}");
    // --- end issue #6 ------------------------------------------------------
    println!("cargo::rerun-if-env-changed=ACADSHARP_NATIVE_DIR");
    // Without this every `cfg(acadsharp_linked)` in the crate is an
    // unexpected_cfgs warning, and the gate denies warnings.
    println!("cargo::rustc-check-cfg=cfg(acadsharp_linked)");

    let manifest_dir = PathBuf::from(env_var("CARGO_MANIFEST_DIR"));
    let header_path = manifest_dir.join(HEADER);
    let header_bytes = std::fs::read(&header_path).unwrap_or_else(|e| {
        panic!(
            "I could not read the vendored header at {}: {e}. It is committed, so this means the \
             checkout is incomplete rather than that something needs fetching.",
            header_path.display()
        )
    });

    let digest = sha256::sha256(&header_bytes);
    verify_digest(&manifest_dir.join(HEADER_DIGEST), &sha256::hex(&digest));

    let header_text = String::from_utf8(header_bytes)
        .expect("the vendored header is not valid UTF-8, which no version of it has ever been");
    let abi_version = u32_define(&header_text, "VIPRS_ACAD_ABI_VERSION");
    let wire_version = u32_define(&header_text, "VIPRS_ACAD_WIRE_VERSION");
    // The fingerprint is the first eight bytes of the digest, big-endian, and
    // that definition lives in the header's own comment on
    // `viprs_acad_abi_fingerprint`.
    let fingerprint =
        u64::from_be_bytes(digest[..8].try_into().expect("a sha256 digest is 32 bytes"));

    let out = PathBuf::from(env_var("OUT_DIR")).join("abi_constants.rs");
    let generated = format!(
        r#"// Generated by build.rs from {HEADER}. Nothing here is typed by hand, so
// editing this file has no effect: the next build overwrites it from the
// header's bytes.

/// The VIPRS CAD ABI version the vendored header declares.
///
/// The native library reports the same number through
/// [`viprs_acad_abi_version`](crate::ffi::viprs_acad_abi_version), and a
/// disagreement is a hard refusal rather than something to negotiate at
/// runtime.
pub const EXPECTED_ABI_VERSION: u32 = {abi_version};

/// The batch protocol version the vendored header declares.
///
/// This one moves on its own. A record's payload can gain a field without a
/// single declaration in the header changing, so a consumer that only calls
/// the entry points is unaffected while a consumer that parses the stream is
/// not. Read both numbers, never infer either from the other.
pub const EXPECTED_WIRE_VERSION: u32 = {wire_version};

/// The first eight bytes of the vendored header's sha256, read big-endian.
///
/// The native library reports the same number through
/// [`viprs_acad_abi_fingerprint`](crate::ffi::viprs_acad_abi_fingerprint).
/// They differ exactly when the header and the library came from different
/// commits, which is the failure worth catching: the library loads, every
/// symbol resolves, and a struct field is four bytes from where this crate
/// believes it is.
pub const EXPECTED_ABI_FINGERPRINT: u64 = {fingerprint:#018x};
"#
    );
    std::fs::write(&out, generated)
        .unwrap_or_else(|e| panic!("I could not write {}: {e}", out.display()));

    // --- issue #6 (H3.4) ---------------------------------------------------
    // The declaration, checked against the header it declares. All three
    // numbers handed over here came out of the header's bytes a few lines up,
    // so this is the declaration being checked and never the header.
    let declared = compat::Compat::read(&manifest_dir.join(COMPAT));
    declared.check_against_header(
        abi_version,
        wire_version,
        &sha256::hex(&digest),
        HEADER,
        COMPAT,
    );
    // And against the archive, if this build has one. Before `resolve_archive`
    // rather than inside it, so an archive the declaration does not cover
    // stops the build before a single link directive is printed, and so that
    // issue #3 can replace the whole of the archive half below without this
    // check going with it.
    check_archive_against_declaration(&declared);
    // --- end issue #6 ------------------------------------------------------

    resolve_archive();
}

// --- issue #6 (H3.4) -------------------------------------------------------

/// Refuses an archive `COMPAT.toml` has not declared this crate compatible
/// with.
///
/// It resolves `ACADSHARP_NATIVE_DIR` itself rather than borrowing whatever
/// the archive half below found, which is one duplicated `env::var_os` and
/// buys a clean seam: issue #3 rewrites `resolve_archive` wholesale and this
/// function is not in it. A missing variable or a missing manifest is silence
/// here, because the archive half already warns about both and two warnings
/// for one absent archive reads like two problems.
fn check_archive_against_declaration(declared: &compat::Compat) {
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        return;
    };
    let manifest = PathBuf::from(&dir).join("metadata").join("LINKINFO.json");
    if !manifest.is_file() {
        return;
    }
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", manifest.display()));
    // Issue #3's manifest parser replaces `from_manifest_text`; `check_archive`
    // takes the two fields as a struct so that swap is a one-line one.
    let identity = compat::ArchiveIdentity::from_manifest_text(&text, &manifest);
    declared.check_archive(&identity, &manifest, COMPAT);
}

// --- end issue #6 ----------------------------------------------------------

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|e| panic!("cargo did not set {name}: {e}"))
}

/// Refuses to build when the vendored header is not the file the committed
/// digest describes.
///
/// This is the whole point of committing the digest. The header is the
/// contract and an edit to it that nobody noticed is an edit to the contract
/// that nobody noticed, so it fails here rather than three layers down as a
/// field reading its neighbour's bytes.
fn verify_digest(path: &Path, actual: &str) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", path.display()));
    let committed = text
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("{} is empty", path.display()));
    assert!(
        committed == actual,
        "the vendored header {HEADER} hashes to {actual}, and {} says it should hash to {committed}. \
         Either the header was edited without re-pinning it, or the pin was moved without copying \
         the header. Fix whichever one it is; do not update the digest to match.",
        path.display()
    );
}

/// One `#define NAME <decimal>` from the header, as the `u32` the generated
/// constants are typed as.
///
/// The parsing is `build/header.rs`, which `tests/` compiles too, so both
/// sides of the cross-check read the header the same way.
fn u32_define(header: &str, name: &str) -> u32 {
    let value = header::integer_define(header, name, HEADER);
    u32::try_from(value).unwrap_or_else(|_| {
        panic!("`#define {name} {value}` in {HEADER} does not fit a u32, and the header's own field for it is 32 bits wide")
    })
}

// ---------------------------------------------------------------------------
// Archive resolution
//
// Issue #3 owns everything below this line and replaces it wholesale. What is
// here is the least that lets the native tests run: shared linking, one field
// read out of the manifest, and a cfg so everything that calls the library can
// compile out when there is no library.
// ---------------------------------------------------------------------------

fn resolve_archive() {
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        warn_no_archive("ACADSHARP_NATIVE_DIR is unset");
        return;
    };
    let root = PathBuf::from(&dir);
    // Canonical, because the rpath below has to survive being read by a loader
    // in a different working directory than the one cargo built in.
    let root = root.canonicalize().unwrap_or(root);
    let lib_dir = root.join("lib");
    let manifest = root.join("metadata").join("LINKINFO.json");

    // Before anything at all is printed, because every line below carries the
    // root and the one that says "no archive resolved" carries it too. A
    // newline in `ACADSHARP_NATIVE_DIR` splits whichever line it lands in, and
    // the second half is a directive the caller chose.
    linkinfo::check_archive_root(&root, &manifest);

    if !lib_dir.is_dir() {
        warn_no_archive(&format!("{} has no lib/ directory", root.display()));
        return;
    }
    if !manifest.is_file() {
        warn_no_archive(&format!("{} has no metadata/LINKINFO.json", root.display()));
        return;
    }
    println!("cargo::rerun-if-changed={}", manifest.display());

    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("I could not read {}: {e}", manifest.display()));
    let system_libraries = linkinfo::system_libraries(&text, "shared_system_libraries", &manifest);

    // Where a downstream build script picks these up, through cargo's `links`
    // key: `DEP_ACADSHARP_NATIVE_NATIVE_DIR` and `DEP_ACADSHARP_NATIVE_LIB_DIR`.
    // Emitted after the control-character check above, like everything else
    // carrying the root.
    println!("cargo::metadata=native_dir={}", root.display());
    println!("cargo::metadata=lib_dir={}", lib_dir.display());

    println!("cargo::rustc-link-search=native={}", lib_dir.display());
    // `dylib=` spelled out rather than left to the default it already is. This
    // half of the build script links shared and only shared, and issue #3 adds
    // the static recipe beside it, so both branches say which one they are.
    println!("cargo::rustc-link-lib=dylib=acadsharp_native");
    for name in system_libraries {
        println!("cargo::rustc-link-lib={name}");
    }

    // An rpath, because without it none of this runs, and it belongs to the
    // shared branch above and to nothing else.
    //
    // Issue #3 inherits that sentence. `lib/` holds both the `.so` and the
    // `.a`, and a bare `-l` picks the `.so`, so an rpath left on the static
    // path means a binary that was supposed to be self-contained links, loads
    // and runs correctly on the build machine by quietly using the shared
    // library. That is the silent success the link contract warns about, and
    // the assertion that catches it is on the binary (`readelf -d` showing no
    // `NEEDED` and no `RUNPATH`) rather than on the answer the library gives
    // back.
    //
    // `-l acadsharp_native` against a directory holding both a `.so` and a
    // `.a` picks the `.so`, and cargo does not put a build script's
    // `rustc-link-search` path on the loader's path: I printed
    // LD_LIBRARY_PATH from a test runner and it holds `target/debug`,
    // `target/debug/deps` and the toolchain's own lib dirs, nothing else. So
    // the test binary linked, and then died at startup with "libacadsharp
    // _native.so: cannot open shared object file". That failure is a 127 from
    // the loader before `main`, which is a long way from looking like a
    // missing search path.
    //
    // This is scoped to this package's own binaries, tests and examples, so it
    // reaches exactly what needs it and nothing a consumer builds. A consumer
    // linking this crate shared has the same problem and needs its own answer,
    // which is issue #3's static recipe.
    println!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());

    println!("cargo::rustc-cfg=acadsharp_linked");
}

fn warn_no_archive(why: &str) {
    println!(
        "cargo::warning=no native archive resolved ({why}), so nothing is linked and every test \
         that calls the library is compiled out of this build."
    );
}
