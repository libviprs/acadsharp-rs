//! Proof that a declared count never becomes an allocation, and that the layer
//! above it allocates only what it hands back.
//!
//! The instrument is a counting global allocator that records the largest
//! single request made while it is armed. RSS is the wrong instrument here and
//! the issue's suggestion to read `/proc/self/statm` does not work: Linux
//! overcommits, so a reservation nobody touches moves RSS by exactly zero and
//! the measurement comes back green for a decoder that asked for 68 GB.
//!
//! This runs with `harness = false` so nothing else is executing while the
//! counter is armed. A second test thread allocating in the measurement window
//! would make the number mean nothing.

mod wire;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use acadsharp_rs::batch::{BatchError, BatchReader, Reason};

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static MAX_SINGLE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static REQUESTS: AtomicUsize = AtomicUsize::new(0);

/// Touches three atomics and allocates nothing, so the instrument cannot
/// disturb what it is measuring.
fn note(size: usize) {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    MAX_SINGLE.fetch_max(size, Ordering::Relaxed);
    TOTAL.fetch_add(size, Ordering::Relaxed);
    REQUESTS.fetch_add(1, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[derive(Debug, Clone, Copy)]
struct Measurement {
    max_single: usize,
    total: usize,
    requests: usize,
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Measurement) {
    MAX_SINGLE.store(0, Ordering::Relaxed);
    TOTAL.store(0, Ordering::Relaxed);
    REQUESTS.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::SeqCst);
    let value = f();
    ARMED.store(false, Ordering::SeqCst);
    (
        value,
        Measurement {
            max_single: MAX_SINGLE.load(Ordering::Relaxed),
            total: TOTAL.load(Ordering::Relaxed),
            requests: REQUESTS.load(Ordering::Relaxed),
        },
    )
}

/// Walks a batch to the end without keeping anything, which is what a decoder
/// that allocates from a declared count would betray itself doing.
fn drain(bytes: &[u8]) -> (usize, Option<BatchError>) {
    match BatchReader::new(bytes) {
        Err(e) => (0, Some(e)),
        Ok(reader) => {
            let mut ok = 0usize;
            for record in reader.records() {
                match record {
                    Ok(_) => ok += 1,
                    Err(e) => return (ok, Some(e)),
                }
            }
            (ok, None)
        }
    }
}

const BUDGET: usize = 1024;

/// The safe layer, which unlike [`acadsharp_rs::batch`] really does allocate:
/// items are owned, and the batch buffer is a `Vec` this crate keeps.
///
/// So the claim here is different. `batch` allocates nothing; `PrimitiveStream`
/// allocates the buffer it was asked for and the items it hands back, and
/// nothing else. Two things would break that and neither would fail any other
/// test in the suite: a per-record allocation that scales with a count a record
/// declares, and anything the stream keeps across batches.
#[cfg(acadsharp_linked)]
fn the_safe_layer(failures: &mut usize) {
    use acadsharp_rs::{Decoder, Document, Limits, PrimitiveStream};

    /// `VIPRSSYN` with one view and twelve primitives, the same probe input
    /// every other native test in this crate opens.
    fn synthetic() -> Vec<u8> {
        let mut bytes = b"VIPRSSYN".to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes
    }

    fn walk(document: &Document) -> (usize, usize) {
        let mut stream: PrimitiveStream<'_> = document.decode(0).expect("view 0 decodes");
        let items = (&mut stream).filter(Result::is_ok).count();
        assert!(stream.is_complete(), "the walk has to be a complete one");
        (items, stream.buffer_len())
    }

    // Everything is built before the counter is armed, so the measurement sees
    // the walk and nothing else. The handshake and the open both allocate, and
    // neither of them is what this is about.
    let bytes = synthetic();
    let roomy = Decoder::new().expect("the handshake passes against the pinned archive");
    let roomy = Document::open_bytes(&roomy, &bytes, &Limits::new()).expect("it opens");
    let cramped = Decoder::new()
        .expect("the handshake passes")
        .with_initial_batch_bytes(12);
    let cramped = Document::open_bytes(&cramped, &bytes, &Limits::new()).expect("it opens");

    let ((items, buffer), first) = measure(|| walk(&roomy));
    println!(
        "PrimitiveStream at the default buffer: {items} items, buffer {buffer} bytes, max single \
         {} bytes, {} bytes over {} requests",
        first.max_single, first.total, first.requests
    );
    if items != 17 {
        eprintln!("FAIL: the walk produced {items} items and the probe emits 17");
        *failures += 1;
    }
    if first.max_single != buffer {
        eprintln!(
            "FAIL: the largest single request during the walk was {} bytes and the batch buffer \
             is {buffer}, so something other than the buffer asked for the biggest allocation",
            first.max_single
        );
        *failures += 1;
    }

    // The same walk again. A stream that kept anything across batches, or a
    // document that accumulated per decode, would cost more the second time.
    let ((again, _), second) = measure(|| walk(&roomy));
    println!(
        "the same walk a second time: {again} items, max single {} bytes, {} bytes over {} \
         requests",
        second.max_single, second.total, second.requests
    );
    if (second.total, second.requests) != (first.total, first.requests) {
        eprintln!(
            "FAIL: the second identical walk allocated {} bytes over {} requests and the first \
             allocated {} over {}, so something accumulates",
            second.total, second.requests, first.total, first.requests
        );
        *failures += 1;
    }

    // And with a twelve byte starting buffer, where the stream has to grow. The
    // growth is to exactly the size the library named, never a doubling and
    // never a reservation from a count a record declared, so no single request
    // comes anywhere near the 64 KiB the roomy run spent in one go.
    let ((items, buffer), tight) = measure(|| walk(&cramped));
    println!(
        "PrimitiveStream from a twelve byte buffer: {items} items, buffer grew to {buffer} bytes, \
         max single {} bytes, {} bytes over {} requests",
        tight.max_single, tight.total, tight.requests
    );
    if items != 17 {
        eprintln!("FAIL: the cramped walk produced {items} items and the probe emits 17");
        *failures += 1;
    }
    if tight.max_single >= BUDGET {
        eprintln!(
            "FAIL: growing from twelve bytes asked for {} bytes in one go, budget is {BUDGET}",
            tight.max_single
        );
        *failures += 1;
    }
    if buffer >= BUDGET {
        eprintln!(
            "FAIL: the buffer reached {buffer} bytes for a document the library packs into \
             batches of a few hundred, so it grew by something other than what it was told"
        );
        *failures += 1;
    }
}

/// With no archive there is nothing to walk, and `tests/native_lane_is_live.rs`
/// is what stops that gate quietly staying shut in a job meant to open it.
#[cfg(not(acadsharp_linked))]
fn the_safe_layer(_failures: &mut usize) {
    println!("PrimitiveStream: skipped, this build linked no library");
}

fn main() {
    // Everything is built before the counter is armed, so the measurement sees
    // the parse and nothing else.
    let wrap = wire::Builder::new()
        .record(wire::polyline_u32_wrap())
        .build();
    let golden: &[u8] = include_bytes!("data/syn_1v_12p.bin");
    let big: &[u8] = include_bytes!("data/syn_3v_40p.bin");
    let mut failures = 0usize;

    // The positive control comes first. If the instrument sees nothing here it
    // is broken, and every zero below would be meaningless.
    let (control, m) = measure(|| {
        let v: Vec<u8> = Vec::with_capacity(4096);
        v.capacity()
    });
    println!(
        "control: a deliberate 4 KiB Vec -> max single {} bytes, {} bytes over {} requests",
        m.max_single, m.total, m.requests
    );
    assert!(control >= 4096);
    if m.max_single < 4096 {
        eprintln!("FAIL: the counting allocator did not see a 4096 byte request, so it is broken");
        failures += 1;
    }

    // The vector: a 72 byte record claiming 2,863,311,531 vertices, which is
    // 68 GB of them. It must be refused, and it must be refused before
    // anything is asked of the allocator.
    let (result, m) = measure(|| drain(&wrap));
    println!(
        "u32 wrap vector: max single {} bytes, {} bytes over {} requests",
        m.max_single, m.total, m.requests
    );
    match result {
        (0, Some(BatchError::CorruptInput { offset: 12, reason })) => {
            if reason != Reason::PolylineLengthMismatch {
                eprintln!("FAIL: the wrap vector was refused for {reason:?}, not its length");
                failures += 1;
            }
        }
        other => {
            eprintln!("FAIL: the wrap vector was not refused at offset 12: {other:?}");
            failures += 1;
        }
    }
    if m.max_single >= BUDGET {
        eprintln!(
            "FAIL: parsing the wrap vector asked for {} bytes in one go, budget is {BUDGET}",
            m.max_single
        );
        failures += 1;
    }

    // The same measurement over a stream that is entirely well formed, where
    // the counts are real and the decoder has every excuse to allocate.
    for (name, bytes) in [("syn_1v_12p.bin", golden), ("syn_3v_40p.bin", big)] {
        let (result, m) = measure(|| {
            let mut offset = 0;
            let mut count = 0usize;
            while offset < bytes.len() {
                let reader = match BatchReader::new(&bytes[offset..]) {
                    Ok(r) => r,
                    Err(_) => return None,
                };
                for record in reader.records() {
                    if record.is_err() {
                        return None;
                    }
                    count += 1;
                }
                offset += reader.total_len();
            }
            Some(count)
        });
        println!(
            "{name}: {result:?} records, max single {} bytes, {} bytes over {} requests",
            m.max_single, m.total, m.requests
        );
        if result.is_none() {
            eprintln!("FAIL: {name} did not parse");
            failures += 1;
        }
        if m.max_single >= BUDGET {
            eprintln!(
                "FAIL: {name} asked for {} bytes in one go, budget is {BUDGET}",
                m.max_single
            );
            failures += 1;
        }
    }

    // Ten thousand passes over the same capture. A decoder that accumulated
    // anything across records would show it here as a total that grows with
    // the pass count rather than staying flat.
    let (passes, m) = measure(|| {
        let mut count = 0usize;
        for _ in 0..10_000 {
            let (ok, _) = drain(golden);
            count += ok;
        }
        count
    });
    println!(
        "10,000 passes over syn_1v_12p.bin: {passes} records, max single {} bytes, {} bytes over {} requests",
        m.max_single, m.total, m.requests
    );
    if m.total >= BUDGET {
        eprintln!(
            "FAIL: 10,000 passes allocated {} bytes in total, so something accumulates",
            m.total
        );
        failures += 1;
    }

    the_safe_layer(&mut failures);

    if failures > 0 {
        eprintln!("{failures} allocation checks failed");
        std::process::exit(1);
    }
    println!("all allocation checks passed");
}
