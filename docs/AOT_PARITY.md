# AOT parity: the NativeAOT shim against the JIT recording

libviprs-dep recorded the expectations under `acadsharp/tests/expectations/` with its JIT fixture generator, over the same flattener sources the NativeAOT shim compiles. `examples/dump.rs` prints this crate's view of a drawing in the format that recording uses, byte for byte, and `tools/aot_parity.sh` diffs the two. With the flattener held constant, a difference that survives is the runtime (trimming, globalization, the wire's encoding) and not the flattener.

Everything here was run on MARS (native x86_64, rust:1 in a container, `CARGO_TARGET_DIR=/tmp/target`), against the fixtures and recordings of libviprs-dep `bc351a9`, which is on its `main`. 45 fixtures carry a recording, and 7 do not.

## How a verdict is made

The recording is six-decimal text with a fixed line shape, and the rule it was recorded under is string equality, so `diff` is the comparator and the first differing line is the first differing record. There is no Rust comparator here and nothing from acadrust.

There are two verdict columns because the wire cannot carry one thing the recording holds. A Warning in the recording has `flags=1` when it was raised inside an expanded insertion, because the recording was dumped from the flattener's in-memory primitives. The wire's Warning record is code, entity handle and message, with no flags field, so a decode through the shim can never print that 1.

- `strict` is a plain diff.
- `wire` is the same diff with `flags=N` on Warning lines set to 0 on both sides. It is the verdict that isolates the runtime.

Every one of the `strict`-only differences below is that one thing, and the first differing record shows it.

To reproduce, with an archive unpacked at `$ARCHIVE` and a libviprs-dep checkout at `bc351a9` in `$DEP`:

```sh
export ACADSHARP_NATIVE_DIR=$ARCHIVE ACADSHARP_REQUIRE_NATIVE=1
cargo build --example dump
tools/aot_parity.sh target/debug/examples/dump \
  $DEP/acadsharp/tests/fixtures $DEP/acadsharp/tests/expectations /tmp/parity-out
```

## Lane 1: the published archive

| | |
| --- | --- |
| artifact_version | 3.7.1-viprs.1 |
| asset | `acadsharp-linux-x64.tgz` from release `acadsharp-3.7.1-viprs.1`, sha256 `4cbdbbdd03c4431780ddca80a1d660e6940ff561734d666e162d6fb50438136e` |
| target | x86_64-unknown-linux-gnu |
| shim_sha256 | not recorded in this archive's metadata, so the asset digest above is the label |
| flattener | the one `viprs.1` shipped, which predates `bc351a9` |

Result: 30 of 45 match under `wire` (26 strictly).

The recording is of `bc351a9`. Between `viprs.1` (`8bf5ad2`) and `bc351a9` the flattener gained SOLID and MESH polygons, 3DFACE, WIPEOUT, MLINE and LEADER lowering, and new refusal paths. So this lane is expected to DIFF on every fixture touching those kinds, and the DIFFs below are a flattener-version difference, listed by kind, and say nothing about JIT against AOT:

| kind | fixtures |
| --- | --- |
| SOLID and 3DFACE as polygons | `g13_solid`, `g13_face3d` |
| MESH, polyface and polygon meshes | `g13_mesh`, `g13_mesh_bad_faces`, `g13_polyface_mesh`, `g13_polygon_mesh` |
| WIPEOUT | `g13_wipeout` |
| MLINE | `g13_mline` |
| LEADER | `g13_leader` |
| RAY and XLINE refusal | `g13_ray_xline` |
| refusal paths | `g13_unsupported` |
| dimension blocks | `g13_dimension`, `g13_dimension_shallow` |
| the two real drawings (SOLID polygons from record 39 and 46) | `real_AC1018`, `real_AC1032` |

| fixture | strict | wire | first differing record, or note |
| --- | --- | --- | --- |
| `g11_codepage` | MATCH | MATCH |  |
| `g11_shapes` | MATCH | MATCH |  |
| `g13_arc` | MATCH | MATCH |  |
| `g13_bad_extents` | MATCH | MATCH |  |
| `g13_circle` | MATCH | MATCH |  |
| `g13_deep_blocks` | MATCH | MATCH |  |
| `g13_dimension` | DIFF | DIFF | record 00009 expected: 00009 Polygon handle=53 flags=1 n=3 closed=1 pts=[(0.000000,3.000000,0.000000);(0.180000,3.030000 |
| `g13_dimension_shallow` | DIFF | DIFF | record 00009 expected: 00009 Polygon handle=53 flags=1 n=3 closed=1 pts=[(0.000000,3.000000,0.000000);(0.180000,3.030000 |
| `g13_ellipse` | MATCH | MATCH |  |
| `g13_empty_view` | MATCH | MATCH |  |
| `g13_face3d` | DIFF | DIFF | record 00004 expected: 00004 Polygon handle=49 flags=0 n=4 closed=1 pts=[(0.000000,0.000000,0.000000);(10.000000,1.00000 |
| `g13_hatch` | MATCH | MATCH |  |
| `g13_insert` | MATCH | MATCH |  |
| `g13_leader` | DIFF | DIFF | record 00004 expected: 00004 Warning handle=49 flags=0 code=ARROWHEAD_NOT_DRAWN message="LEADER asks for an arrowhead at |
| `g13_line` | MATCH | MATCH |  |
| `g13_long_text` | MATCH | MATCH |  |
| `g13_mesh` | DIFF | DIFF | record 00004 expected: 00004 Warning handle=49 flags=0 code=MESH_SUBDIVISION_IGNORED message="MESH carries subdivision l |
| `g13_mesh_bad_faces` | DIFF | DIFF | record 00004 expected: 00004 Polygon handle=49 flags=0 n=3 closed=1 pts=[(0.000000,0.000000,0.000000);(10.000000,0.00000 |
| `g13_mirrored_bulge` | MATCH | MATCH |  |
| `g13_mline` | DIFF | DIFF | record 00004 expected: 00004 Polyline handle=4C flags=0 n=3 closed=0 pts=[(0.000000,2.500000,0.000000);(7.500000,2.50000 |
| `g13_nan_bulge` | MATCH | MATCH |  |
| `g13_nonuniform` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4C flags=1 code=NON_UNIFORM_BLOCK_SCALE message="CIRCLE crossed under a bloc |
| `g13_ocs_mirror` | MATCH | MATCH |  |
| `g13_ocs_plane` | MATCH | MATCH |  |
| `g13_ocs_rotated` | MATCH | MATCH |  |
| `g13_ocs_skew` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4C flags=1 code=NON_UNIFORM_BLOCK_SCALE message="CIRCLE crossed under a bloc |
| `g13_point` | DIFF | MATCH | record 00008 expected: 00008 Warning handle=50 flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this ve |
| `g13_polyface_mesh` | DIFF | DIFF | record 00004 expected: 00004 Warning handle=49 flags=0 code=UNSUPPORTED_ENTITY message="POLYFACE_MESH is not a primitive |
| `g13_polygon_mesh` | DIFF | DIFF | record 00004 expected: 00004 Warning handle=49 flags=0 code=UNSUPPORTED_ENTITY message="POLYGON_MESH is not a primitive  |
| `g13_polyline` | MATCH | MATCH |  |
| `g13_ray_xline` | DIFF | DIFF | record 00005 expected: 00005 Warning handle=4A flags=0 code=ENTITY_REFUSED_BY_DESIGN message="RAY runs from its base poi |
| `g13_scale_1x` | MATCH | MATCH |  |
| `g13_slot` | MATCH | MATCH |  |
| `g13_slot_block` | MATCH | MATCH |  |
| `g13_solid` | DIFF | DIFF | record 00004 expected: 00004 Polygon handle=49 flags=0 n=4 closed=1 pts=[(0.000000,0.000000,0.000000);(10.000000,1.00000 |
| `g13_spline` | MATCH | MATCH |  |
| `g13_text` | MATCH | MATCH |  |
| `g13_tolerance` | DIFF | MATCH | record 00006 expected: 00006 Warning handle=4E flags=1 code=UNSUPPORTED_ENTITY message="TOLERANCE is not a primitive thi |
| `g13_two_entities` | MATCH | MATCH |  |
| `g13_unsupported` | DIFF | DIFF | record 00005 expected: 00005 Polygon handle=4A flags=0 n=3 closed=1 pts=[(0.000000,0.000000,0.000000);(0.000000,0.000000 |
| `g13_wide_polyline` | MATCH | MATCH |  |
| `g13_wipeout` | DIFF | DIFF | record 00004 expected: 00004 Warning handle=49 flags=0 code=POLYGON_MASKS message="WIPEOUT is a mask, and the Polygon re |
| `g13_xref` | MATCH | MATCH |  |
| `real_AC1018` | DIFF | DIFF | record 00039 expected: 00039 Polygon handle=322 flags=0 n=4 closed=1 pts=[(75.039418,8.050392,0.000000);(80.721042,8.050 |
| `real_AC1032` | DIFF | DIFF | record 00046 expected: 00046 Polygon handle=322 flags=0 n=4 closed=1 pts=[(75.039418,8.050392,0.000000);(80.721042,8.050 |
| `g13_ac1009` | UNRECORDED | ERROR | dump: could not open /dep/fixtures/g13_ac1009.dwg: this build does not read that drawing format, it reads AC10xx version |
| `g13_dimension_deep` | UNRECORDED | ERROR | dump: decode of /dep/fixtures/g13_dimension_deep.dwg failed after 0 records: the decode hit a bound set in Limits |
| `g13_many_inserts` | UNRECORDED | UNRECORDED | 30004 records, opened |
| `g13_scale_16x` | UNRECORDED | UNRECORDED | 2052 records, opened |
| `g13_scale_4x` | UNRECORDED | UNRECORDED | 516 records, opened |
| `g13_wide_spline` | UNRECORDED | UNRECORDED | 5 records, opened |
| `g13_xref_long` | UNRECORDED | UNRECORDED | 6 records, opened |

## Lane 2: the archive rebuilt from `bc351a9`

| | |
| --- | --- |
| artifact_version | 3.7.1-viprs.2 (the rebuilt one calls itself that, so the commit is the label) |
| viprs_dep_commit | `bc351a906d9085f256fb2226fd6050f72e2d3325` |
| asset | `acadsharp-linux-x64.tgz` as built here, sha256 `9d1632f0766e4966a5a9fbd1cc23e9c65d93aab6eba52d7eccfefd0d7f473249` (a rebuild is not promised to be byte-identical, so this digest labels this build only) |
| target | x86_64-unknown-linux-gnu |
| shared library | `lib/libacadsharp_native.so`, sha256 `670cd1e78931c51e31db28878875cb8e9dc923dc0a6ff6c8f4ef64d9c828197a` |
| builder | debian:bookworm-slim, .NET SDK 10.0.401, clang 14.0.6, GNU ld 2.40, `invariant_globalization: true` |
| provenance | `tests/data/linkinfo/x86_64-unknown-linux-gnu.3.7.1-viprs.2.json` |

Result: 45 of 45 match under `wire` (31 strictly). The flattener is the recording's own, so this is the lane that isolates the runtime, and it says the NativeAOT shim reproduces the JIT recording record for record on all 45 recorded fixtures, the two real drawings included, once the one field the wire does not carry is set aside.

| fixture | strict | wire | first differing record, or note |
| --- | --- | --- | --- |
| `g11_codepage` | MATCH | MATCH |  |
| `g11_shapes` | MATCH | MATCH |  |
| `g13_arc` | MATCH | MATCH |  |
| `g13_bad_extents` | MATCH | MATCH |  |
| `g13_circle` | MATCH | MATCH |  |
| `g13_deep_blocks` | MATCH | MATCH |  |
| `g13_dimension` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4D flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this ve |
| `g13_dimension_shallow` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4D flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this ve |
| `g13_ellipse` | MATCH | MATCH |  |
| `g13_empty_view` | MATCH | MATCH |  |
| `g13_face3d` | MATCH | MATCH |  |
| `g13_hatch` | MATCH | MATCH |  |
| `g13_insert` | MATCH | MATCH |  |
| `g13_leader` | DIFF | MATCH | record 00009 expected: 00009 Warning handle=50 flags=1 code=ARROWHEAD_NOT_DRAWN message="LEADER asks for an arrowhead at |
| `g13_line` | MATCH | MATCH |  |
| `g13_long_text` | MATCH | MATCH |  |
| `g13_mesh` | DIFF | MATCH | record 00007 expected: 00007 Warning handle=4D flags=1 code=MESH_SUBDIVISION_IGNORED message="MESH carries subdivision l |
| `g13_mesh_bad_faces` | MATCH | MATCH |  |
| `g13_mirrored_bulge` | MATCH | MATCH |  |
| `g13_mline` | MATCH | MATCH |  |
| `g13_nan_bulge` | MATCH | MATCH |  |
| `g13_nonuniform` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4C flags=1 code=NON_UNIFORM_BLOCK_SCALE message="CIRCLE crossed under a bloc |
| `g13_ocs_mirror` | MATCH | MATCH |  |
| `g13_ocs_plane` | MATCH | MATCH |  |
| `g13_ocs_rotated` | MATCH | MATCH |  |
| `g13_ocs_skew` | DIFF | MATCH | record 00004 expected: 00004 Warning handle=4C flags=1 code=NON_UNIFORM_BLOCK_SCALE message="CIRCLE crossed under a bloc |
| `g13_point` | DIFF | MATCH | record 00008 expected: 00008 Warning handle=50 flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this ve |
| `g13_polyface_mesh` | DIFF | MATCH | record 00006 expected: 00006 Warning handle=60 flags=1 code=UNSUPPORTED_ENTITY message="POLYFACE_MESH is not a primitive |
| `g13_polygon_mesh` | DIFF | MATCH | record 00006 expected: 00006 Warning handle=68 flags=1 code=UNSUPPORTED_ENTITY message="POLYGON_MESH is not a primitive  |
| `g13_polyline` | MATCH | MATCH |  |
| `g13_ray_xline` | DIFF | MATCH | record 00007 expected: 00007 Warning handle=4F flags=1 code=ENTITY_REFUSED_BY_DESIGN message="RAY runs from its base poi |
| `g13_scale_1x` | MATCH | MATCH |  |
| `g13_slot` | MATCH | MATCH |  |
| `g13_slot_block` | MATCH | MATCH |  |
| `g13_solid` | MATCH | MATCH |  |
| `g13_spline` | MATCH | MATCH |  |
| `g13_text` | MATCH | MATCH |  |
| `g13_tolerance` | DIFF | MATCH | record 00006 expected: 00006 Warning handle=4E flags=1 code=UNSUPPORTED_ENTITY message="TOLERANCE is not a primitive thi |
| `g13_two_entities` | MATCH | MATCH |  |
| `g13_unsupported` | MATCH | MATCH |  |
| `g13_wide_polyline` | MATCH | MATCH |  |
| `g13_wipeout` | DIFF | MATCH | record 00010 expected: 00010 Warning handle=4F flags=1 code=POLYGON_MASKS message="WIPEOUT is a mask, and the Polygon re |
| `g13_xref` | MATCH | MATCH |  |
| `real_AC1018` | DIFF | MATCH | record 00216 expected: 00216 Warning handle=62C flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this v |
| `real_AC1032` | DIFF | MATCH | record 00223 expected: 00223 Warning handle=62C flags=1 code=UNSUPPORTED_ENTITY message="POINT is not a primitive this v |
| `g13_ac1009` | UNRECORDED | ERROR | dump: could not open /dep/fixtures/g13_ac1009.dwg: this build does not read that drawing format, it reads AC10xx version |
| `g13_dimension_deep` | UNRECORDED | ERROR | dump: decode of /dep/fixtures/g13_dimension_deep.dwg failed after 0 records: the decode hit a bound set in Limits |
| `g13_many_inserts` | UNRECORDED | UNRECORDED | 30004 records, opened |
| `g13_scale_16x` | UNRECORDED | UNRECORDED | 2052 records, opened |
| `g13_scale_4x` | UNRECORDED | UNRECORDED | 516 records, opened |
| `g13_wide_spline` | UNRECORDED | UNRECORDED | 5 records, opened |
| `g13_xref_long` | UNRECORDED | UNRECORDED | 6 records, opened |

The seven fixtures with no recording are opened and counted only. Two do not decode, and both are the library refusing on purpose: `g13_ac1009` is an R12 drawing and the library reads AC10xx 1014 to 1032, and `g13_dimension_deep` is the fixture that hits the block depth bound.

## Rebuilding the archive

`tests/data/linkinfo/REBUILD.md` has the command, so the suite's emitter lane (R3.1) runs the same one.

## What this does not show

Only linux x64. The macOS dylib, arm64 and the static link were not run here. The seven unrecorded fixtures are not compared with anything, and `flags=` is printed from the one bit the safe API exposes (expanded from a nested insertion), which is all the recordings use.
