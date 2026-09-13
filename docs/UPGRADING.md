# Moving this crate onto a new ACadSharp

This is the crate's own runbook. It covers everything in `acadsharp-rs` that
has to move when ACadSharp or the native shim does, in the order that lets each
step be checked before the next one depends on it.

It does not cover the suite side. `acadsharp-rs-tests` owns the oracle, the
frozen outputs and the parity inventory, and the upgrade gate that regenerates
and diffs them is its workflow rather than this crate's. ACadSharp reaches this
crate only as a published `libviprs-dep` archive, so nothing here installs a
.NET SDK, restores from NuGet, or builds upstream from source.

## The short version

Five things move, they live in four files, and three of the five are checked
against something else. That last number is the reason this page exists: it
used to be two, and the two that were not checked were the two that drift.

| What moves | Where | What catches it being wrong |
| --- | --- | --- |
| The header's bytes | `native/viprs_acadsharp.h` | its digest, below |
| The header's digest | `native/viprs_acadsharp.h.sha256` | `build.rs` refuses a mismatch on every build |
| Where the header came from | `native/NATIVE_HEADER_REV` | **nothing**, see below |
| The archive CI fetches | `.github/actions/fetch-native-archive/action.yml` | its own sha256, and now `COMPAT.toml` |
| The compatibility declaration | `COMPAT.toml` | `build.rs` against the header, and against the archive's manifest |

`README.md`'s compatibility table moves too, and it is generated, so it counts
as part of `COMPAT.toml` rather than as a sixth thing.

## Step 0: there has to be a release first

Nothing here fetches ACadSharp and nothing here builds it. The upstream bump
happens in `libviprs-dep`, which builds the NativeAOT shim, runs its own
verifier over the archive, and publishes a release carrying one `.tgz` per
target plus `metadata/LINKINFO.json`, `include/viprs_acadsharp.h`, `ABI.md`,
`WIRE.md` and `LINKINFO.md` inside each one.

Until that release exists there is nothing for this crate to point at, and
pointing at a draft or at an artifact from a workflow run is the same mistake
as pointing at a branch: it moves.

Write down two things from the release before going on, because every step
below wants one of them:

- the release tag, e.g. `acadsharp-3.7.1-viprs.1`;
- the sha256 of the Linux x64 archive, which is the one CI downloads.

## Step 1: revendor the header, if it moved

`native/viprs_acadsharp.h` is a byte-for-byte copy of `include/viprs_acadsharp.h`
from the archive. It is vendored rather than fetched so that a build needs
nothing off the network, and so that `docs.rs` and every archive-free CI job
still generate the same constants.

```sh
tar -xzf acadsharp-linux-x64.tgz
cp <unpacked>/include/viprs_acadsharp.h native/viprs_acadsharp.h
sha256sum native/viprs_acadsharp.h | sed 's#native/##' > native/viprs_acadsharp.h.sha256
```

Then put the `libviprs-dep` commit the header came from into
`native/NATIVE_HEADER_REV`, replacing the sha on the one uncommented line.

**`NATIVE_HEADER_REV` is checked by nothing.** It is provenance: no build reads
it, no test compares it, and a stale one looks exactly like a fresh one. It is
the only thing on this page with that property, so it is the one to do while
you still have the release page open rather than afterwards.

The digest beside the header is a different matter: `build.rs` recomputes it
every build and refuses to run when the two disagree. So an edited header with
a stale digest cannot get past `cargo check`, and neither can a bumped digest
with the old header.

If the header did not move, skip all of this. A shim revision that changes no
declarations publishes the same header bytes, which is what `-viprs.<revision>`
in the artifact version is for.

## Step 2: move the archive pin CI fetches

`.github/actions/fetch-native-archive/action.yml` carries the release tag, the
archive filename and the archive's sha256 as three input defaults, in one place,
used by both the `Test` job and the `Suite (acadsharp-rs-tests)` job. Move all
three together. A tag moved without its digest fails the `sha256sum -c` in the
action, which is the good failure; a digest moved without its tag fails the same
way.

Only the Linux x64 archive is pinned, deliberately. The macOS dylib in these
archives carries an install name (`@rpath/viprs_acadsharp.dylib`) that matches
no file shipping beside it, so nothing that links it can load, and no CI job
here tries.

## Step 3: move `COMPAT.toml`

Five keys, and what each one is checked against is written in the file beside
it. Practically:

- `abi_version` and `wire_version` come from the new header's `#define`s. If
  you guess, the build tells you what the header actually says and names both
  files.
- `abi_header_sha256` is the digest from step 1. Same deal.
- `native_artifact_versions` is a glob over the archive's `artifact_version`.
  A shim-only bump moves the revision (`3.7.1-viprs.1` to `3.7.1-viprs.2`) and
  the glob already covers it. An upstream bump moves the part in front and the
  glob has to move with it.
- `acadsharp_versions` lists the upstream versions this crate has been run
  against. Add the new one; drop the old one only when nothing is expected to
  link an old archive any more.

Then regenerate the README table, which is a single command:

```sh
ACADSHARP_UPDATE_README=1 cargo test --test compat readme
```

### What now checks what, in CI

This is the step that used to be unchecked and is not any more. The `Test` and
`Suite` jobs both unpack the pinned archive and export `ACADSHARP_NATIVE_DIR`
before building, and the build refuses an archive whose `artifact_version`
misses the glob or whose `acadsharp_version` is not in the list, naming
`COMPAT.toml`. So the action's pin and the declaration are now checked against
each other by every run that links anything: bump one without the other and CI
goes red with a message saying which two files disagree.

The archive-free jobs (`Check & Lint`, `MSRV`, `Docs`) have no manifest to read
and check the first three keys only, which is all they can honestly check.

## Step 4: run the tests and read what moved

In Docker, with the archive, because nothing here is built on a host toolchain:

```sh
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

Then read the failures rather than fixing them one at a time, because which
ones failed is the report on what the bump changed:

- `tests/compat.rs` failing is the declaration disagreeing with the header or
  the archive. Fix `COMPAT.toml`.
- `tests/abi_constants.rs` failing is the generated constants disagreeing with
  the header. That is a build script bug or a header this parser cannot read,
  not something to paper over.
- `tests/ffi_layout.rs` failing is a struct that changed shape. The transcription
  in `src/ffi.rs` has to move with it, and this is the failure that would
  otherwise be a field read four bytes from where it lives.
- `tests/ffi_handshake.rs` failing is the library disagreeing with the header it
  shipped beside, which means the archive and the vendored header came from
  different builds. Go back to step 1.
- `tests/golden.rs`, `tests/roundtrip.rs` or `tests/sweeps.rs` failing is the
  decoded output moving. That is the interesting one and it is the diff the
  suite's upgrade gate exists to make reviewable, so it belongs in the PR that
  does the bump rather than in a fix that follows it.

Also run the whole thing once with `ACADSHARP_NATIVE_DIR` unset. Everything that
reaches the library is `cfg(acadsharp_linked)`, so that run proves the crate
still builds, checks and documents for somebody who has no archive at all, which
is what `docs.rs` is.

## Step 5: the pins that point at the other repo

If the bump needs a matching change in `acadsharp-rs-tests`, the landing order
is in `README.md` under "Cross-repo pins and the landing order", and it is five
steps with the first two expected to be red. The one people forget is the last:
a crate PR that moves `SUITE_REV` onto the suite's merge commit. A red
`Suite (acadsharp-rs-tests)` on `main` means exactly that PR is owed.

## What this runbook still cannot check for you

The `acadsharp_versions` list is a claim that this crate has been run against
those versions, and running it is the part no file can assert. The proof is the
suite's frozen outputs and its differential run against the native path, in
`acadsharp-rs-tests`. Adding a version here says somebody did that; it does not
do it.
