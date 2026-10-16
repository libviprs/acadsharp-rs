# What separates a pure-Rust reader from the ACadSharp recording

This is the measurement behind #16, kept in the repo so the numbers do not live only in an issue body. It is evidence for the epic (#7): the question there is whether to wrap ACadSharp's NativeAOT build or read DWG in Rust, and this is what the Rust side looked like on the two real drawings when I ran it.

I did not re-run any of it for this page. The harness lives in a fork of acadrust ([spdrman/acadrust#2](https://github.com/spdrman/acadrust/pull/2)), outside this repo, and the numbers below are what that run reported against libviprs-dep's recordings at commit `0d475bf`. If a number here matters to a decision, re-run the harness first.

## The headline

[acadrust](https://github.com/hakanaktt/acadrust) 0.5.5 agrees with the recording on 44 of the 46 fixtures that carry one, 39 of them exactly. The two real drawings, `real_AC1018.dwg` and `real_AC1032.dwg`, came out at 318 records against the recording's 380. That net 62 is really 74 missing and 12 spurious (identical on both drawings: 318 - 12 = 306 matched, 380 - 306 = 74).

| Cause | Records | Why |
| --- | --- | --- |
| TABLE not expanded | 55 missing | acadrust gives both tables an empty `block_name` and a populated `block_record_handle`. The harness resolves blocks by name, so nothing under a table gets placed. ACadSharp models `TableEntity : Insert`, so its adapter reaches them through the INSERT path. |
| ATTDEF inside inserted blocks | 9 missing | The harness places them but has no renderer. The adapter's `case TextEntity` catches ATTDEF and emits its default value. |
| MLINE | 6 missing | Not rendered. |
| INSERT attributes (ATTRIB) | 4 missing | They hang off `Insert.attributes` and are not in the document's entity list. |
| INSERT inside a DIMENSION block, expanded | 12 spurious | The harness expands them, the adapter skips them (`Flattener.cs:1455`). |

My earlier guess was that MLINE and hatch loops carrying a curve explained most of the gap. They explain 6 of 74, and hatch explains none: neither drawing holds a single `HATCH_LOOP_NOT_POLYGON` warning. TABLE is 74% of what is missing and was never on my list.

## Three things that are decisions, not work

1. The adapter drops dimension arrowhead geometry on purpose (a dimension block is generated geometry, and walking it as a block would give it a second depth budget), so `_ArchTick` and `_BoxBlank` are never drawn. The 12 "spurious" records are real tick geometry the Rust reader reads correctly. The depth budget is a good reason, and losing the arrowheads is the price. I would record it as a known, accepted gap in the flattener rather than reopen it here, and it is the flattener's call, not this crate's.
2. The adapter draws an ATTDEF and its ATTRIB at the same point (handle `6F8` holds `"1"` and handle `705` holds `"17"`, both at `(920.679627, 16.352854)`). AutoCAD shows only the instance for a non-constant attribute. Matching the recording means reproducing the overdraw, so any comparison against it has to.
3. A multi-line attribute (handle `79D`) is empty at AC1032 and whole at AC1018. I first called this an ACadSharp defect. libviprs-dep#112 later traced it to the adapter never reading the field that holds the text on R2018, so it is a flattener bug and not an upstream one, and it does not favour the Rust reader the way I first wrote.

## What would close the gap, by records per unit of effort

1. Skip INSERT members when expanding a dimension block: 12 records, about 2 lines, if decision 1 goes that way.
2. Expand TABLE by block handle rather than name: 55 records, about 15 lines.
3. Render ATTDEF as Text: 9 records, about 10 lines.
4. Emit INSERT attributes: 4 records, about 20 lines.
5. MLINE: 6 records, about 50 lines. The adapter's formula reproduces all 6 polylines from acadrust's data to 1e-6, so the data is there and the cost is the mitre and the style lookup.

## Two more things the measurement turned up

- 10 DIFFs sit behind the count gap. Of 306 matched keys, 299 bodies are identical. Six differ only because the recording escapes backslashes, and four more will differ because the harness prints a raw newline where the recording writes a literal `\n`. Neither already-known acadrust difference (un-normalised normals, non-finite geometry) shows up in these two drawings.
- The harness's hatch model is wrong in a way these fixtures do not catch. `TryPolygon` lowers a `Hatch.BoundaryPath.Arc` edge to a bulged polygon, and only ellipse, spline and full-circle edges take the warning route, while the harness treats any non-Line/Polyline edge as uncomparable and drops bulges from Polyline edges. No fixture here exercises it, which is a finding of its own.

## How this relates to the AOT parity run

`docs/AOT_PARITY.md` (issue #20) answers a different question: it holds the flattener constant and compares the NativeAOT build with the JIT recording. This page is about a different reader against that same recording. Between them, the recording is the shared reference.
