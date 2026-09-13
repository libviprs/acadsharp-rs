# acadsharp-rs

A safe, idiomatic Rust decoder for DWG, backed by the ACadSharp NativeAOT
artifacts published by [`libviprs-dep`](https://github.com/libviprs/libviprs-dep).

Callers get Rust types. Neither .NET nor ACadSharp internals reach them.

## Status

Early. What exists is the raw `ffi` module, a byte-for-byte copy of the frozen
`viprs_acadsharp.h` under `native/`, and `abi`, which holds the constants
generated from that header and the handshake that refuses a library built
against a different one. The safe API on top of it is still being built.

`abi::check` compares the two numbers and is always there. `abi::handshake` is
the wrapper that asks the library for them, so it exists only in a build that
linked one (and in the documentation, which is how it stays visible on
docs.rs).

The ABI version, the wire version and the fingerprint are generated from the
vendored header's bytes at build time, so none of the three is typed anywhere in
Rust source. `native/NATIVE_HEADER_REV` names the `libviprs-dep` commit the
header came from and `native/viprs_acadsharp.h.sha256` pins its contents; the
build refuses to run if the header and that digest disagree.

## Requirements

- **Rust 1.97+** (edition 2024)

Nothing else to build it. The native library arrives as a published release
artifact rather than as a sibling checkout, so building or using this crate
needs no .NET SDK and no second repo on disk. CI does lay the test suite down
beside this checkout, which is a different thing and is described below.

To link the library and run the tests that call it, unpack a `libviprs-dep`
archive and point `ACADSHARP_NATIVE_DIR` at the directory holding `lib/` and
`metadata/LINKINFO.json`. Without that the crate still builds, checks and
documents, and everything that reaches the library is compiled out.

## Boundary

This crate owns the safe Rust API and the FFI that reaches the native library.

It does not own the native build, the C ABI definition, or the artifact release
mechanics, all of which live in `libviprs-dep`. It also does not own CAD
normalisation, MVT or PMTiles, which live in `libviprs`.

## Cross-repo pins and the landing order

The test suite lives in
[`acadsharp-rs-tests`](https://github.com/libviprs/acadsharp-rs-tests) and
depends on this crate by path, so the two repos pin each other by sha and
neither of them guesses a branch:

- `SUITE_REV` here names the `acadsharp-rs-tests` commit that the
  `Suite (acadsharp-rs-tests)` CI job runs against this tree.
- `COUNTERPART_REV` there names the `acadsharp-rs` commit that the suite's own
  gate builds against.

Both files are one 40-character lowercase sha under a block of comments. A
branch name in either of them is always wrong: guessing a same-named counterpart
branch is how `libviprs`' integration job can end up building against the wrong
tree (libviprs#1013).

### A change that needs both repos

Land it in this order, and expect the first two steps to be red:

1. Open the suite PR. It stays red, because its `COUNTERPART_REV` cannot point
   at a crate change that has not merged yet.
2. Open the crate PR with `SUITE_REV` set to the head of that suite PR. The
   `Suite (acadsharp-rs-tests)` job prints a `::notice` saying the pin is not on
   the suite's `main` yet, and stays green. On a PR branch an unmerged pin is
   the only way a paired change can be tested at all.
3. The crate PR goes green and merges.
4. The suite PR bumps `COUNTERPART_REV` to the crate's merge commit, goes green
   and merges.
5. A one-line crate PR moves `SUITE_REV` onto the suite's merge commit.

Step 5 is not optional. Between step 3 and step 5, `SUITE_REV` on `main` names a
commit that is not on the suite's `main`, and on `main` that same `::notice`
becomes a failure. **A red `Suite (acadsharp-rs-tests)` on `main` means exactly
one thing: that one-line PR is owed.**

A change that touches only one repo needs none of this. Both pins stay on `main`
commits.

### What the job checks before it trusts a run

A green `Suite (acadsharp-rs-tests)` is only worth something if the suite it ran
was really built against this tree, so the job proves that first and refuses
rather than skips:

- The pin parses as one 40-character lowercase sha, and the checkout that comes
  back is at exactly that sha.
- `cargo metadata` resolves a package called `acadsharp-rs` from this checkout's
  own `Cargo.toml`, with no source (so it is the path dependency, not a registry
  copy), and the suite depends on it as a normal dependency rather than a dev or
  optional one. Grepping the manifest for the dependency would prove only that
  somebody wrote it down: a `[patch]`, a workspace member, `optional = true` or
  an entry under `[dev-dependencies]` all satisfy a text match and change what
  actually gets built.
- `SUITE_REV` obeys the merged-only rule for the ref the run is on.
- The pinned native archive is unpacked and `ACADSHARP_NATIVE_DIR` is exported
  before the suite builds, through `.github/actions/fetch-native-archive`.
  Everything that reaches the native library is `cfg(acadsharp_linked)`, so a
  suite run without the archive compiles every native test out and goes green
  having run none of them.

There is no `if:` and no `continue-on-error` anywhere in the job, and
`tests/suite_pin_check.rs` fails if either one appears: a skipped check is the
same colour as a passing one.

## Licence

MIT. The ACadSharp licence and third-party notices ship with the native
artifacts rather than with this crate.
