# acadsharp-rs

A safe, idiomatic Rust decoder for DWG, backed by the ACadSharp NativeAOT
artifacts published by [`libviprs-dep`](https://github.com/libviprs/libviprs-dep).

Callers get Rust types. Neither .NET nor ACadSharp internals reach them.

## Status

Early. What exists is the raw `ffi` module, a byte-for-byte copy of the frozen
`viprs_acadsharp.h` under `native/`, and the handshake that refuses a library
built against a different header. The safe API on top of it is still being
built.

The ABI version, the wire version and the fingerprint are generated from the
vendored header's bytes at build time, so none of the three is typed anywhere in
Rust source. `native/NATIVE_HEADER_REV` names the `libviprs-dep` commit the
header came from and `native/viprs_acadsharp.h.sha256` pins its contents; the
build refuses to run if the header and that digest disagree.

## Requirements

- **Rust 1.97+** (edition 2024)

Nothing else to build it. The native library arrives as a published release
artifact rather than as a sibling checkout, so there is no counterpart repo to
lay down beside this one and no .NET SDK needed.

To link it and run the tests that call it, unpack a `libviprs-dep` archive and
point `ACADSHARP_NATIVE_DIR` at the directory holding `lib/` and
`metadata/LINKINFO.json`. Without that the crate still builds, checks and
documents, and everything that reaches the library is compiled out.

## Boundary

This crate owns the safe Rust API and the FFI that reaches the native library.

It does not own the native build, the C ABI definition, or the artifact release
mechanics, all of which live in `libviprs-dep`. It also does not own CAD
normalisation, MVT or PMTiles, which live in `libviprs`.

## Licence

MIT. The ACadSharp licence and third-party notices ship with the native
artifacts rather than with this crate.
