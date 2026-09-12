# acadsharp-rs

A safe, idiomatic Rust decoder for DWG, backed by the ACadSharp NativeAOT
artifacts published by [`libviprs-dep`](https://github.com/libviprs/libviprs-dep).

Callers get Rust types. Neither .NET nor ACadSharp internals reach them.

## Status

Scaffolding. The crate compiles and the CI gate runs, and that is all so far.
The native ABI it will link is being specified in `libviprs-dep`, and this crate
is the Rust side of that ABI's conformance testing.

## Requirements

- **Rust 1.97+** (edition 2024)

Nothing else. The native library arrives as a published release artifact rather
than as a sibling checkout, so there is no counterpart repo to lay down beside
this one and no .NET SDK needed to build or use this crate.

## Boundary

This crate owns the safe Rust API and the FFI that reaches the native library.

It does not own the native build, the C ABI definition, or the artifact release
mechanics, all of which live in `libviprs-dep`. It also does not own CAD
normalisation, MVT or PMTiles, which live in `libviprs`.

## Licence

MIT. The ACadSharp licence and third-party notices ship with the native
artifacts rather than with this crate.
