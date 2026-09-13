//! The only place outside [`crate::ffi`] where `unsafe` lives.
//!
//! Everything above this module works with owned Rust values and typed
//! errors; everything below it is the C boundary. This is the seam, and it is
//! deliberately thin: it owns the two handles, it fills and zeroes the
//! out-structs, it does the two-call convention for every string, and it turns
//! a result code into a [`crate::Error`]. It makes no decisions of its own.
//!
//! # Why the calls are split in two
//!
//! [`raw`] mirrors the header one declaration at a time and is the half that
//! needs a library. Without an archive its bodies report a bug instead of
//! calling anything, which means the logic in this module compiles, lints and
//! documents in every job rather than only in the one that links. A consumer
//! cannot write `cfg(acadsharp_linked)` themselves, so the public surface
//! stays the same shape either way and the absence of a library arrives as
//! [`crate::Error::Unlinked`] rather than as a missing type. [`LINKED`] is
//! what [`crate::Decoder::new`] asks before anything else.

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use crate::diagnostics;
use crate::error::{Error, Result};
use crate::ffi;
use crate::limits::Limits;
use crate::stream::{BatchSource, NativeBatch};

/// Asks the library which contract it was built against, and refuses it if
/// that is not this one.
///
/// Call this before anything else that touches the library;
/// [`crate::Decoder::new`] is the only thing that does. The comparison is
/// [`crate::abi::check`], which lives up there because it is policy over two
/// integers and is worth compiling, linting and testing in the three CI jobs
/// that link nothing. Fetching the two integers is a call into the library, so
/// it lives down here with every other one.
///
/// The unlinked half is the whole reason [`Error::Unlinked`] exists: a
/// consumer cannot write `cfg(acadsharp_linked)` themselves, so the absence of
/// a library arrives as a value they can match on rather than as a type that
/// is not there.
#[cfg(acadsharp_linked)]
pub(crate) fn handshake() -> Result<()> {
    // SAFETY: `viprs_acad_abi_version` takes no arguments, returns a plain
    // `uint32_t`, touches no memory the caller owns and is documented never to
    // fail. The only precondition on it is that the symbol is linked, which is
    // what the `cfg` above says.
    let actual_version = unsafe { ffi::viprs_acad_abi_version() };
    // SAFETY: the same, for a `uint64_t`.
    let actual_fingerprint = unsafe { ffi::viprs_acad_abi_fingerprint() };

    crate::abi::check(actual_version, actual_fingerprint).map_err(Error::HeaderMismatch)
}

/// See the linked half above.
#[cfg(not(acadsharp_linked))]
pub(crate) fn handshake() -> Result<()> {
    Err(Error::Unlinked)
}

/// Fills in the bounds a caller set, leaving every other field zero, which is
/// what the boundary reads as "you pick".
fn limits_struct(limits: &Limits) -> Result<ffi::viprs_acad_limits_v1> {
    // The one field the header declares narrower than the rest. Every bound is
    // an `Option<u64>` up above, so that a later widening of this field is not
    // a change to the public API, and a value that cannot fit today is a
    // refusal rather than a silent truncation to something much smaller.
    let max_block_depth = match limits.max_block_depth() {
        None => 0,
        Some(depth) => u32::try_from(depth).map_err(|_| Error::InvalidArgument)?,
    };

    Ok(ffi::viprs_acad_limits_v1 {
        max_input_bytes: limits.max_input_bytes().unwrap_or(0),
        max_entities: limits.max_entities().unwrap_or(0),
        max_string_bytes: limits.max_string_bytes().unwrap_or(0),
        max_polyline_points: limits.max_polyline_points().unwrap_or(0),
        max_block_depth,
        max_output_bytes: limits.max_output_bytes().unwrap_or(0),
        // `struct_size` and `struct_version`, which the callee refuses if they
        // are zero. Everything else above is a bound, and a zero bound is what
        // the boundary reads as "you pick".
        ..Default::default()
    })
}

/// The header, one declaration at a time, with the linkage question answered
/// once.
///
/// Every function here has the header's signature. The linked half forwards;
/// the unlinked half writes nothing and reports
/// [`ffi::VIPRS_ACAD_INTERNAL_ERROR`], which is unreachable because
/// [`crate::Decoder::new`] refuses first and every handle in the crate comes
/// from a `Decoder`.
mod raw {
    #[cfg(acadsharp_linked)]
    pub(super) use crate::ffi::{
        viprs_acad_close as close, viprs_acad_decode_begin as decode_begin,
        viprs_acad_decode_close as decode_close, viprs_acad_decode_next_batch as next_batch,
        viprs_acad_get_capabilities_v1 as get_capabilities,
        viprs_acad_get_view_info_v1 as get_view_info, viprs_acad_open_memory as open_memory,
        viprs_acad_open_path_utf8 as open_path, viprs_acad_view_count as view_count,
    };

    #[cfg(not(acadsharp_linked))]
    mod unlinked {
        use crate::ffi;

        pub(in crate::sys) unsafe fn get_capabilities(
            _out: *mut ffi::viprs_acad_capabilities_v1,
            _version_utf8: *mut u8,
            _cap: u64,
            _required: *mut u64,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn open_path(
            _path: *const u8,
            _path_len: u64,
            _limits: *const ffi::viprs_acad_limits_v1,
            _out: *mut *mut ffi::viprs_acad_handle,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn open_memory(
            _data: *const u8,
            _data_len: u64,
            _limits: *const ffi::viprs_acad_limits_v1,
            _out: *mut *mut ffi::viprs_acad_handle,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn view_count(
            _h: *mut ffi::viprs_acad_handle,
            _out_count: *mut u32,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn get_view_info(
            _h: *mut ffi::viprs_acad_handle,
            _index: u32,
            _out: *mut ffi::viprs_acad_view_info_v1,
            _name_utf8: *mut u8,
            _name_cap: u64,
            _name_required: *mut u64,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn decode_begin(
            _h: *mut ffi::viprs_acad_handle,
            _view_index: u32,
            _cancel_flag: *const u32,
            _out: *mut *mut ffi::viprs_acad_decode_handle,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn next_batch(
            _d: *mut ffi::viprs_acad_decode_handle,
            _buf: *mut u8,
            _cap: u64,
            _written: *mut u64,
            _done: *mut u8,
        ) -> u32 {
            ffi::VIPRS_ACAD_INTERNAL_ERROR
        }

        pub(in crate::sys) unsafe fn decode_close(_d: *mut ffi::viprs_acad_decode_handle) {}

        pub(in crate::sys) unsafe fn close(_h: *mut ffi::viprs_acad_handle) {}
    }

    #[cfg(not(acadsharp_linked))]
    pub(super) use unlinked::{
        close, decode_begin, decode_close, get_capabilities, get_view_info, next_batch,
        open_memory, open_path, view_count,
    };
}

/// Takes [`Send`] and [`Sync`] away from whatever holds one.
///
/// A real null pointer rather than a `PhantomData<*const ()>`, which the type
/// system cannot tell apart and a reader of the compiler's refusal can: a
/// `PhantomData` chain points at core's own `marker.rs`, and the expected
/// output of a compile-fail test that names a file inside the toolchain is
/// expected output that goes stale every time the toolchain is rebuilt.
pub(crate) struct NotThreadSafe(
    // Never read, and that is the point: the field is here for its type.
    #[allow(dead_code)] *const (),
);

impl NotThreadSafe {
    pub(crate) const fn new() -> Self {
        Self(ptr::null())
    }
}

/// The drawing-version range a refusal of code 2 is told to look at. Every
/// call carries it so the error it builds is complete where it is built.
pub(crate) type Dwg = (u32, u32);

/// Turns a result code into a refusal, or into nothing.
fn check(code: u32, dwg: Dwg) -> Result<()> {
    match Error::from_native_code(code, dwg.0, dwg.1) {
        None => Ok(()),
        Some(error) => Err(error),
    }
}

/// The two-call string convention, in one place.
///
/// A caller sizes a buffer by calling once with a null pointer and a capacity
/// of zero, reads `required`, allocates and calls again. There is no
/// terminator in either direction, so the length is the whole of it, and a
/// buffer shorter than the string is written into not at all rather than
/// partially: a partial UTF-8 string can end mid sequence and nothing
/// downstream could tell that from a complete one.
fn read_string(dwg: Dwg, mut call: impl FnMut(*mut u8, u64, *mut u64) -> u32) -> Result<String> {
    let mut required: u64 = 0;
    // The sizing call. A real null with a capacity of zero, never the dangling
    // pointer an empty `Vec` hands out: a null buffer with a non-zero capacity
    // is a caller who has confused the two, and the boundary says so.
    check(call(ptr::null_mut(), 0, &raw mut required), dwg)?;

    let needed = usize::try_from(required).map_err(|_| Error::Internal {
        what: "the library asked for a string longer than this host can hold",
    })?;
    if needed == 0 {
        return Ok(String::new());
    }

    let mut bytes = vec![0u8; needed];
    let mut again: u64 = 0;
    check(call(bytes.as_mut_ptr(), required, &raw mut again), dwg)?;
    if again != required {
        return Err(Error::Internal {
            what: "the library asked for one string length and then wrote another",
        });
    }

    String::from_utf8(bytes).map_err(|_| Error::Internal {
        what: "the library wrote a string that is not UTF-8, which this boundary promises",
    })
}

/// A build's capabilities, with no C type left in them.
///
/// `sys` is the only module that names an `ffi` type, so what leaves it is
/// plain owned data. That is not tidiness: it is what makes "no `ffi` type is
/// reachable from the public API" something `tests/api_surface.rs` can check
/// by reading the modules above this one.
pub(crate) struct RawCapabilities {
    pub(crate) abi_version: u32,
    pub(crate) wire_version: u32,
    pub(crate) dwg_version_min: u32,
    pub(crate) dwg_version_max: u32,
    pub(crate) supports_block_expansion: u8,
    pub(crate) supports_warnings: u8,
    pub(crate) acadsharp_version: String,
}

/// One view, with no C type left in it.
pub(crate) struct RawView {
    pub(crate) index: u32,
    pub(crate) kind: u32,
    pub(crate) extents: [f64; 4],
    pub(crate) entity_count: u64,
    pub(crate) name: String,
}

/// What a build can do, plus the pinned version of the backing reader.
pub(crate) fn capabilities() -> Result<RawCapabilities> {
    // Nothing knows the drawing range yet, which is fine: the capabilities
    // call is not one that can report an unsupported drawing format.
    let dwg = (0, 0);
    let mut out = ffi::viprs_acad_capabilities_v1::default();

    let version = read_string(dwg, |buf, cap, required| {
        // SAFETY: `out` is a live, fully initialised struct of the type the
        // callee expects, with `struct_size` and `struct_version` set as the
        // header requires. `buf` is either a real null with a capacity of zero
        // or a live allocation of exactly `cap` bytes, and `required` is a
        // live `u64` the callee may write.
        unsafe { raw::get_capabilities(&raw mut out, buf, cap, required) }
    })?;

    Ok(RawCapabilities {
        abi_version: out.abi_version,
        wire_version: out.wire_version,
        dwg_version_min: out.dwg_version_min,
        dwg_version_max: out.dwg_version_max,
        supports_block_expansion: out.supports_block_expansion,
        supports_warnings: out.supports_warnings,
        acadsharp_version: version,
    })
}

/// An open document, released by [`ffi::viprs_acad_close`] on the way out.
///
/// Holding the raw pointer is also what makes every type above this one
/// neither [`Send`] nor [`Sync`], with no `unsafe impl` anywhere: one decode
/// handle is single threaded and calls on it must not overlap, so the absence
/// is the correct answer rather than an oversight.
pub(crate) struct DocumentHandle {
    ptr: *mut ffi::viprs_acad_handle,
}

impl DocumentHandle {
    fn as_ptr(&self) -> *mut ffi::viprs_acad_handle {
        self.ptr
    }
}

impl Drop for DocumentHandle {
    fn drop(&mut self) {
        diagnostics::note_document_close();
        // SAFETY: the pointer came back non-null from an open call that
        // returned OK, nothing else closed it (this type is not `Clone` and
        // owns the only copy), and the boundary takes a close on a handle it
        // issued, or a null, and cannot fail.
        unsafe { raw::close(self.ptr) };
    }
}

/// A decode in progress over one view, released by
/// [`ffi::viprs_acad_decode_close`].
pub(crate) struct DecodeHandle {
    ptr: *mut ffi::viprs_acad_decode_handle,
    /// The cancel flag the library was pointed at, kept alive here because it
    /// must outlive the decode handle it was given to.
    _cancel: Option<Arc<AtomicU32>>,
}

impl Drop for DecodeHandle {
    fn drop(&mut self) {
        diagnostics::note_decode_close();
        // SAFETY: same as the document handle. Closing the decode first is
        // what the borrow on `PrimitiveStream<'doc>` guarantees, because
        // `viprs_acad_close` would otherwise have released this handle
        // already.
        unsafe { raw::decode_close(self.ptr) };
    }
}

impl BatchSource for DecodeHandle {
    fn next_batch(&mut self, buf: &mut [u8]) -> NativeBatch {
        let mut written: u64 = 0;
        let mut done: u8 = 0;
        let cap = buf.len() as u64;

        // SAFETY: `self.ptr` is a decode handle this library issued and has
        // not been closed, `buf` is a live allocation of exactly `cap` bytes
        // that nothing else aliases while this call runs (it comes from
        // `&mut`), and both out parameters are live and zeroed. The two are
        // read below only for the codes that write them.
        let code = unsafe {
            raw::next_batch(
                self.ptr,
                buf.as_mut_ptr(),
                cap,
                &raw mut written,
                &raw mut done,
            )
        };

        NativeBatch {
            code,
            written,
            done,
        }
    }
}

/// Opens a document from a filesystem path.
///
/// The path crosses as UTF-8 bytes and a length. Nothing here reads the file:
/// that is the library's job, and doing it twice would mean holding a copy of
/// every drawing in memory for no reason.
pub(crate) fn open_path(path: &[u8], limits: &Limits, dwg: Dwg) -> Result<DocumentHandle> {
    let limits = limits_struct(limits)?;
    let mut handle: *mut ffi::viprs_acad_handle = ptr::null_mut();
    // SAFETY: `path` is a live slice for the duration of the call, `limits` is
    // a live struct of the type the callee expects with its size and version
    // set, and `handle` is a live out-pointer. The library owns none of them
    // after the call returns.
    let code = unsafe {
        raw::open_path(
            path.as_ptr(),
            path.len() as u64,
            &raw const limits,
            &raw mut handle,
        )
    };
    finish_open(code, handle, dwg)
}

/// Opens a document from caller-owned bytes, which the library reads during
/// the call and never keeps.
pub(crate) fn open_memory(bytes: &[u8], limits: &Limits, dwg: Dwg) -> Result<DocumentHandle> {
    let limits = limits_struct(limits)?;
    let mut handle: *mut ffi::viprs_acad_handle = ptr::null_mut();
    // SAFETY: as above. The buffer is borrowed for the call and the boundary
    // documents that it keeps nothing.
    let code = unsafe {
        raw::open_memory(
            bytes.as_ptr(),
            bytes.len() as u64,
            &raw const limits,
            &raw mut handle,
        )
    };
    finish_open(code, handle, dwg)
}

fn finish_open(code: u32, ptr: *mut ffi::viprs_acad_handle, dwg: Dwg) -> Result<DocumentHandle> {
    check(code, dwg)?;
    if ptr.is_null() {
        return Err(Error::Internal {
            what: "an open call reported success and handed back nothing",
        });
    }
    Ok(DocumentHandle { ptr })
}

/// How many views the document holds.
pub(crate) fn view_count(document: &DocumentHandle, dwg: Dwg) -> Result<u32> {
    let mut count: u32 = 0;
    // SAFETY: the handle is live and `count` is a live `u32` the callee writes
    // on success.
    let code = unsafe { raw::view_count(document.as_ptr(), &raw mut count) };
    check(code, dwg)?;
    Ok(count)
}

/// One view's struct and its name.
pub(crate) fn view_info(document: &DocumentHandle, index: u32, dwg: Dwg) -> Result<RawView> {
    let mut out = ffi::viprs_acad_view_info_v1::default();
    let name = read_string(dwg, |buf, cap, required| {
        // SAFETY: the handle is live, `out` is a live fully initialised struct
        // with its size and version set, and the buffer is either a real null
        // with a capacity of zero or a live allocation of exactly `cap` bytes.
        unsafe { raw::get_view_info(document.as_ptr(), index, &raw mut out, buf, cap, required) }
    })?;
    Ok(RawView {
        index: out.index,
        kind: out.kind,
        // Verbatim, in the order the header declares them. Whether they are a
        // box anybody can use is a question for `View::extents`, and it is
        // asked in exactly one place.
        extents: [out.min_x, out.min_y, out.max_x, out.max_y],
        entity_count: out.entity_count,
        name,
    })
}

/// Begins decoding one view.
pub(crate) fn decode_begin(
    document: &DocumentHandle,
    view_index: u32,
    cancel: Option<Arc<AtomicU32>>,
    dwg: Dwg,
) -> Result<DecodeHandle> {
    // `AtomicU32` has the same size and bit validity as `u32`, and the
    // allocation is the `Arc`'s, so this address is stable for as long as the
    // handle below keeps its clone. Never the address of a local: a flag that
    // moved would leave the decoder reading a dead stack slot.
    let flag: *const u32 = match cancel.as_ref() {
        Some(shared) => Arc::as_ptr(shared).cast::<u32>(),
        None => ptr::null(),
    };

    let mut handle: *mut ffi::viprs_acad_decode_handle = ptr::null_mut();
    // SAFETY: the document handle is live, `flag` is either null or the
    // address of a live `AtomicU32` the returned handle keeps alive, and
    // `handle` is a live out-pointer.
    let code = unsafe { raw::decode_begin(document.as_ptr(), view_index, flag, &raw mut handle) };
    check(code, dwg)?;
    if handle.is_null() {
        return Err(Error::Internal {
            what: "decode_begin reported success and handed back nothing",
        });
    }
    Ok(DecodeHandle {
        ptr: handle,
        _cancel: cancel,
    })
}
