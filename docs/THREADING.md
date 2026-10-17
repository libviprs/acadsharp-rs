# Threads, cancellation and what gets freed

This page says exactly what the crate promises about threads, about stopping a
decode, and about handing memory back. Every sentence under "Guaranteed" has a
test in `acadsharp-rs-tests` that fails if the sentence stops being true, and
the test is named next to it. Where there is no test, the page says so.

The tests run against the real NativeAOT library, in the suite's native lane
(`--features native`, `ACADSHARP_NATIVE_DIR` set). Nothing here is exercised by
this crate's own stub-driven tests, which prove the logic around the calls and
not the library behind them.

## One decoder, one thread

`Decoder`, `Document` and `PrimitiveStream` are neither `Send` nor `Sync`. A
handle belongs to the thread that made it, and a handle is never used from two
threads at once, because the type system will not let you write the program.

* The crate's own compile-fail cases (`tests/compile_fail/`, run by `trybuild`)
  fail to build any program that moves or shares one of the three.
* `acadsharp-rs-tests/tests/threading.rs` asserts the same from outside the
  crate, with a probe that is itself checked against a type known to be `Send`
  and one known not to be, so a probe that always answered "no" cannot make it
  pass. A future `unsafe impl Send` in this crate fails that suite as well as
  this one.

The reason is `ABI.md`'s Threading section: one decode handle is single threaded,
calls on it must not overlap, and nothing is re-entrant. The crate does not add a
lock to hide that, so the honest wording is "one decoder, one thread".

## What threads may do

Independent handles on independent threads are fine, because the ABI says so:
"Two handles may be used from two threads, including two decode handles on the
same document." In this crate that means each thread builds its **own**
`Decoder`, opens its **own** `Document` and decodes with its own stream.

`acadsharp-rs-tests/tests/threading.rs` decodes two different drawings on two
threads at once, a hundred times, and compares every output with the
single-threaded decode of the same file. It also decodes the same drawing from
two threads, twenty rounds. A library that was not safe for this would corrupt
output or crash, and the comparison is what would show it.

`CancelToken` is the one type meant to cross threads. It is `Send + Sync`, it is
cheap to clone, and `cancel` is a release store.

## Cancellation is observed between batches

A `CancelToken` passed to `Document::decode_with_cancel` is read by the library
before each batch, never in the middle of a native parse, and never written by
it. Once it is set the next pull answers `Error::Cancelled`, and the answer is
final: clearing the flag does not resume the stream, and the stream is fused.

The bound, counted in batches and not in time, is: if the flag is visible when
`PrimitiveStream::batches_pulled()` reads `k`, the stream fails at pull `k + 1`
at the latest. The items already decoded out of batch `k` come out first, then
the next pull is the refusal.

* A token set before `decode_with_cancel` makes the first item `Cancelled` and
  delivers no records (`tests/cancellation.rs`).
* A token set from another thread while the stream is mid-stream fails within
  that bound, on the fixture with the most batches (`tests/cancellation.rs`).
* Dropping the stream and the document after a cancel releases everything (the
  hundred-round loop in `tests/cancellation.rs`).

**Not guaranteed, and not tested because it cannot be:** a cancel is not noticed
inside one native batch. A decode that is slow inside a single batch stays slow
until that batch ends. There is no seam inside a native parse to raise a flag
from.

## Giving memory back

Dropping a `PrimitiveStream` closes its decode handle, and dropping a `Document`
closes the document and any decode handle still open on it. That holds on the
paths that do not reach the end of the stream: abandoning a decode after its
first batch, a decode that was cancelled, and an `open` that failed.

`acadsharp-rs-tests/tests/resource_semantics.rs` runs each of those a thousand
times and compares the process's resident set after the loop with the size after
the tenth iteration, with a margin stated in the test. A document that is never
closed is far outside the margin: measured on native x86_64 Linux against
`acadsharp-3.7.1-viprs.1`, taking the close out of `Document`'s `Drop` grew the
process by about 2.9 GiB over a thousand iterations, against a few MiB of noise
when it is there. The same loops run under LeakSanitizer in the suite's
`Native (linux-x64, lsan)` cell, which compares two loop lengths because the
NativeAOT runtime keeps a constant handful of startup allocations until exit.
With one extra Rust allocation leaked per decode handle on a scratch copy, the
leaked total grew from 124 allocations at 20 iterations to 1804 at 300, and the
cell fails on exactly that growth.

What the two checks cannot see, so nobody reads more into them:

* **A decode handle that is never closed.** With `viprs_acad_decode_close`
  taken out of the stream's `Drop` on a scratch copy, neither the RSS loops nor
  LeakSanitizer noticed: a decode handle is small, and the library frees it with
  its document regardless. The close on that path is held by this crate's own
  stub-driven tests and not by the native suite, and this page does not claim
  more.
* **The .NET GC heap**, for LeakSanitizer. It sees Rust allocations and the
  library's `malloc`, not managed memory. RSS covers the whole process but is a
  blunt instrument: it cannot see a leak smaller than its margin.

Neither one validates the ABI. They say the crate closes what it opens, on the
paths they exercise.
