# acadsharp-rs

A safe, idiomatic Rust decoder for DWG, backed by the ACadSharp NativeAOT
artifacts published by [`libviprs-dep`](https://github.com/libviprs/libviprs-dep).

Callers get Rust types. Neither .NET nor ACadSharp internals reach them.

## Using it

```rust
use acadsharp_rs::{Decoder, Document, Item, Limits, Primitive};

fn main() -> Result<(), acadsharp_rs::Error> {
    // The handshake, once. A library built from a different header than the
    // one this crate vendored is refused here, rather than four bytes into
    // a struct that looks plausible.
    let decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        // Nothing was linked. A consumer cannot write that cfg themselves,
        // so it arrives as a value rather than as a missing type.
        Err(error) if error.is_unlinked() => return Ok(()),
        Err(error) => return Err(error),
    };
    println!("ACadSharp {}", decoder.capabilities().acadsharp_version());

    // `VIPRSSYN` is the library's own synthetic document, so this example
    // needs no drawing file. For a real one use
    // `Document::open_path(&decoder, "plan.dwg", &limits)`, which hands the
    // path across as bytes and never reads the file in Rust.
    let limits = Limits::new().with_max_polyline_points(100_000);
    let document = Document::open_bytes(&decoder, b"VIPRSSYN", &limits)?;

    for view in document.views()? {
        println!("view {} is {:?}, called {}", view.index(), view.kind(), view.name());
    }

    let mut lines = 0usize;
    let mut stream = document.decode(0)?;
    for item in &mut stream {
        match item? {
            Item::Primitive(Primitive::Line(line)) => {
                lines += 1;
                let _ = (line.start, line.end);
            }
            Item::Warning(warning) => println!("{}: {}", warning.code, warning.message),
            _ => {}
        }
    }

    // The totals are the only proof the decode was not truncated, so ask
    // rather than trusting that the loop ended for a good reason.
    assert!(stream.is_complete());
    println!("{lines} lines out of {} records", stream.records_seen());
    Ok(())
}
```

`Decoder` runs the ABI handshake once and reads what the build can do.
`Document` owns an open drawing and hands out `View`s. `PrimitiveStream` walks
one view, pulling one native batch at a time into a buffer the caller never
sees. Nothing accumulates across batches, no raw pointer, no `ffi` type and no
`unsafe` is reachable from any of it, and none of the three handles is `Send`
or `Sync`: one decode handle is single threaded and calls on it must not
overlap.

Three things are worth knowing before the first call.

**The totals are the completeness proof.** A decode that stopped early yields
its error and then `None`, and `None` on its own looks exactly like an ending.
`PrimitiveStream::is_complete` compares what came out with what the stream's
own `DocumentEnd` says should have.

**Warnings are data, in the stream.** A decode that emits a hundred of them and
finishes succeeded. They arrive inline because reading one often means reading
what sits beside it.

**Nothing is tessellated or projected.** Arcs, circles, ellipses and splines
keep their parameters, a polyline's bulges cross as bulges, and every
coordinate is 3D. Turning a curve into segments needs a tolerance and
flattening 3D into a plane needs an axis, and neither is a choice this crate
can make for a consumer it cannot see. `libviprs` makes both downstream.

## Status

The safe API above is in. Underneath it are `batch`, the zero-copy decoder for
the VACB wire protocol, and `abi`, which holds the constants generated from the
frozen `viprs_acadsharp.h` vendored under `native/` and `abi::check`, the
comparison that refuses a library built against a different one. The
transcription of the header itself and every call into the library sit below
both of those and are not part of what this crate promises.

The ABI version, the wire version and the fingerprint are generated from the
vendored header's bytes at build time, so none of the three is typed anywhere
in Rust source. `native/NATIVE_HEADER_REV` names the `libviprs-dep` commit the
header came from and `native/viprs_acadsharp.h.sha256` pins its contents; the
build refuses to run if the header and that digest disagree.

With no archive the crate still builds, checks, tests and documents. The public
surface is the same either way and `Decoder::new` answers `Error::Unlinked`
instead of disappearing, because a consumer cannot write
`cfg(acadsharp_linked)` themselves.

## Requirements

- **Rust 1.97+** (edition 2024)

Nothing else to build it. The native library arrives as a published release
artifact rather than as a sibling checkout, so building or using this crate
needs no .NET SDK and no second repo on disk. CI does lay the test suite down
beside this checkout, which is a different thing and is described below.

To link the library and run the tests that call it, unpack a `libviprs-dep`
archive and point `ACADSHARP_NATIVE_DIR` at the directory holding `lib/` and
`metadata/LINKINFO.json`. Without that the crate still builds, checks and
documents: the public surface is the same either way, the calls underneath it
are what is compiled out, and `Decoder::new` answers `Error::Unlinked`.

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
