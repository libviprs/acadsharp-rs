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

## What this crate is compatible with

`COMPAT.toml` at the root is the declaration, and nothing in it is a fact this
crate owns. Two of the five keys belong to the vendored header, one is that
header's digest, and two describe the archives `libviprs-dep` publishes. So the
build checks every line against the thing it describes and refuses to carry on
when one of them disagrees, which is what keeps the file from becoming
decoration.

<!-- BEGIN generated from COMPAT.toml -->
| What `COMPAT.toml` declares | Value | What checks it |
| --- | --- | --- |
| `abi_version` | `2` | `#define VIPRS_ACAD_ABI_VERSION` in `native/viprs_acadsharp.h`, every build |
| `wire_version` | `2` | `#define VIPRS_ACAD_WIRE_VERSION` in `native/viprs_acadsharp.h`, every build |
| `abi_header_sha256` | `0502ac0f616115300fc52c84d99054e366a7ea520363f166d463b44c506233fa` | the sha256 of `native/viprs_acadsharp.h`, recomputed every build |
| `native_artifact_versions` | `3.7.1-viprs.*` | `artifact_version` in the archive's `metadata/LINKINFO.json`, when one resolves |
| `acadsharp_versions` | `3.7.1` | `acadsharp_version` in the archive's `metadata/LINKINFO.json`, when one resolves |
<!-- END generated from COMPAT.toml -->

The table is generated. `tests/compat.rs` fails when it and `COMPAT.toml`
disagree, and `ACADSHARP_UPDATE_README=1 cargo test --test compat readme`
rewrites it.

"When one resolves" in the last two rows means any archive the build script
finds, whether that is `ACADSHARP_NATIVE_DIR` or the cache under
`$CARGO_HOME/acadsharp-native/`. The check hangs off the archive rather than off
a route to it. The same two rows are also checked with no archive at all, by
reading the release tag CI pins in `.github/actions/fetch-native-archive/action.yml`:
that tag is `acadsharp-<artifact_version>`, so the pin and the declaration meet
as two strings in two committed files and a pin that moved without the
declaration goes red in every job.

The two version numbers are declared there and derived in `build.rs` from the
header's own bytes, and the build stops when the two disagree. That is the
other way round from reading the numbers out of the TOML, on purpose: a header
bump that forgot `COMPAT.toml` would otherwise compile clean against a number
nobody had checked. `docs/UPGRADING.md` walks an ACadSharp bump through every
file that has to move.

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

If you are building a **binary** on top of this crate rather than a library,
read [What a binary that depends on this crate has to do](#what-a-binary-that-depends-on-this-crate-has-to-do)
before you run it: the default shared link leaves the loader with nothing to
find and the failure arrives at startup rather than at link time.

### Where an archive is looked for

Two places, in order, and nowhere else. The build script never downloads
anything.

1. `ACADSHARP_NATIVE_DIR`, pointing at an unpacked archive root.
2. `$CARGO_HOME/acadsharp-native/<artifact_version>/<platform>-<cpu>/`, for
   example `~/.cargo/acadsharp-native/3.7.1-viprs.1/linux-arm64/`. Unpack an
   archive there and the variable becomes unnecessary. With two versions cached
   for one target the build script refuses to guess and asks for the variable.

Fetching an archive automatically is a later feature and deliberately not this
one: a build script that downloads is a build script that behaves differently
on a machine with no network, and the pin, the digest check and the unpack
already live in `.github/actions/fetch-native-archive`.

### Shared or static

`link-static` is a Cargo feature, off by default, and off means the shared link.

There is only one feature on purpose. Cargo features are additive: any crate in
a graph may turn one on and none of them can turn another's off. A `link-shared`
beside this one existed briefly, and a graph with one dependency asking for each
unified both on and stopped the build with advice neither author could act on.
With no feature the plan is already the shared one, so the second feature
carried that hazard and bought nothing.

`link-static` needs an archive whose `metadata/LINKINFO.json` says
`static_certified: true`, which is a measurement rather than an intention: it
is true only where the producer linked the static archive into a probe program
and ran it on that target. Asking for it anywhere else is a refusal naming the
target. `link-static` picks nothing until an archive has resolved, so
`cargo doc --all-features` without one stays green.

### What a binary that depends on this crate has to do

**Static is the mode to deploy in and shared is the mode to develop in.** A
static link puts the library inside the binary and that binary runs anywhere.
The shared link does not, and the reason is worth stating plainly rather than
being discovered.

This crate's build script emits the rpath that finds `libacadsharp_native.so`
as `cargo::rustc-link-arg`, and cargo binds that to the emitting package's own
binaries, tests and examples. It goes no further. So this crate's own tests
link and run, and a binary that depends on this crate links, and then dies
before `main`:

```
error while loading shared libraries: libacadsharp_native.so: cannot open
shared object file: No such file or directory
```

with exit code 127. Three ways out, and the first is the one to reach for:

1. **A `build.rs` of your own, emitting your own rpath.** This crate declares
   `links = "acadsharp_native"`, so cargo hands your build script the archive's
   location:

   ```text
   // build.rs in the crate that produces the binary
   fn main() {
       if let Ok(dir) = std::env::var("DEP_ACADSHARP_NATIVE_LIB_DIR") {
           println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
       }
   }
   ```

   The four variables are `DEP_ACADSHARP_NATIVE_LIB_DIR` (the archive's `lib/`),
   `DEP_ACADSHARP_NATIVE_NATIVE_DIR` (the archive root),
   `DEP_ACADSHARP_NATIVE_LINK_KIND` (`shared` or `static`) and
   `DEP_ACADSHARP_NATIVE_ARTIFACT_VERSION`. They are only set when an archive
   actually resolved, so read them with a `Result` and carry on without them.
   Note that this pins the build machine's path into the binary, which is
   exactly what you want for a developer build and exactly what you do not want
   for one you ship.

2. **`link-static`.** No loader involved, nothing to find at run time, and the
   binary is self-contained. This is the answer for anything that leaves the
   machine that built it.

3. **`LD_LIBRARY_PATH`** pointing at the archive's `lib/`, set wherever the
   binary runs. The quickest thing to type and the easiest to forget on the
   machine that matters.

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
