//! Proof that opening a path does not read the file in Rust.
//!
//! The claim is easy to make and easy to lose: one `std::fs::read` added for
//! convenience turns every open into a copy of the whole drawing, and nothing
//! about the result would look different. So this measures instead of
//! asserting. The instrument is a counting global allocator that records the
//! largest single request while it is armed, and the input is a sparse file
//! far larger than any buffer the open legitimately needs.
//!
//! It runs with `harness = false` because a second test thread allocating
//! inside the measurement window would make the number mean nothing, which is
//! the same reason `tests/alloc_bound.rs` does.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static MAX_SINGLE: AtomicUsize = AtomicUsize::new(0);

fn note(size: usize) {
    if ARMED.load(Ordering::Relaxed) {
        MAX_SINGLE.fetch_max(size, Ordering::Relaxed);
    }
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

#[cfg(acadsharp_linked)]
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    MAX_SINGLE.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    let value = f();
    ARMED.store(false, Ordering::Relaxed);
    (value, MAX_SINGLE.load(Ordering::Relaxed))
}

#[cfg(acadsharp_linked)]
const FILE_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(acadsharp_linked)]
fn run() {
    use std::io::{Seek, SeekFrom, Write};

    use acadsharp_rs::{Decoder, Document, Limits};

    let dir = std::env::temp_dir().join(format!("acadsharp-rs-h31-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("big.dwg");

    {
        let mut file = std::fs::File::create(&path).expect("creating the probe file");
        // `VIPRSSYN` plus one view and three primitives, then a quarter of a
        // gigabyte of hole. The file is sparse, so it costs a few kilobytes on
        // disk and would cost 256 MiB in a Rust buffer.
        file.write_all(b"VIPRSSYN").expect("the magic");
        file.write_all(&1u32.to_le_bytes()).expect("the view count");
        file.write_all(&3u32.to_le_bytes()).expect("the primitives");
        file.seek(SeekFrom::Start(FILE_BYTES - 1)).expect("seeking");
        file.write_all(&[0]).expect("the last byte");
    }
    let on_disk = std::fs::metadata(&path).expect("stat").len();
    assert_eq!(
        on_disk, FILE_BYTES,
        "the probe file is {on_disk} bytes long"
    );

    let decoder = Decoder::new().expect("the handshake passes");
    let limits = Limits::new().with_max_input_bytes(FILE_BYTES * 2);

    let (document, largest) =
        measure(|| Document::open_path(&decoder, &path, &limits).expect("the path route opens it"));

    println!(
        "opening a {} MiB file allocated at most {largest} bytes in one request",
        FILE_BYTES / (1024 * 1024)
    );
    assert!(
        largest < 1024 * 1024,
        "opening a {FILE_BYTES} byte file asked for {largest} bytes in a single allocation, \
         which is what reading the file into a Rust buffer looks like"
    );

    // And the open really did happen: the library sniffed the magic itself.
    let views = document.views().expect("the views");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name(), "Model");

    drop(document);
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
    println!("path_open_does_not_read: ok");
}

#[cfg(not(acadsharp_linked))]
fn run() {
    // Without an archive there is no open to measure. `tests/native_lane_is_live.rs`
    // is what stops a job that was supposed to have one from passing quietly.
    println!("path_open_does_not_read: no archive, nothing to measure");
}

fn main() {
    run();
}
