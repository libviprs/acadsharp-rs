//! The static link, proven by calling through it and by reading the binary.
//!
//! The static recipe has exactly one silent failure. Getting the order of the
//! two `rustc-link-lib` lines wrong fails loudly on `RhRegisterOSModule`, and
//! so does leaving the default `+bundle` on either of them. Omitting
//! `+whole-archive` on the init archive links clean, resolves every symbol, and
//! aborts on the first call into the library, because the .NET runtime's static
//! initialiser defines no global symbol anything references and ordinary
//! archive semantics therefore never pull it in.
//!
//! So a test that only links proves nothing. This one opens the library's own
//! synthetic document and reads the answer back.
//!
//! The second half is about the binary rather than the answer. `lib/` holds
//! both `libacadsharp_native.so` and `libacadsharp_native.a`, and a bare `-l`
//! picks the `.so`. A static link that quietly fell back to the shared library
//! links, loads and runs correctly on the build machine and gives exactly the
//! same right answer, so the answer alone cannot tell the two apart. `readelf
//! -d` can: a real static link has no `NEEDED libacadsharp_native.so` and no
//! `RUNPATH`.

/// Whether this run asked for the static link at all.
///
/// Read through `cfg!` rather than `#[cfg]` so the guard below compiles in
/// every configuration and can fail in the one where it matters.
fn static_was_requested() -> bool {
    cfg!(feature = "link-static")
}

#[test]
fn asking_for_the_static_link_and_getting_one_are_the_same_thing() {
    // The anti-skip rule for this lane. Everything below is
    // `cfg(acadsharp_static_linked)`, which the build script only emits when it
    // actually chose the static plan, so a cell that meant to link statically
    // and did not would run zero of these tests and go green. This is the test
    // that cannot compile out.
    if !static_was_requested() {
        eprintln!("the link-static feature is off, so there is no static link to check");
        return;
    }
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        eprintln!("link-static is on and there is no archive, so nothing was linked either way");
        return;
    };

    let statically_linked = cfg!(acadsharp_static_linked);
    assert!(
        statically_linked,
        "the link-static feature is on and ACADSHARP_NATIVE_DIR is {dir:?}, and \
         `cfg(acadsharp_static_linked)` is absent, so every test in this file compiled out and \
         this run is about to go green without linking the static archive once. Read the build \
         script's warnings: either it found no archive under that directory or it chose the \
         shared plan."
    );
}

#[cfg(acadsharp_static_linked)]
mod linked {
    use std::path::PathBuf;
    use std::process::Command;
    use std::ptr;

    use acadsharp_rs::ffi;

    /// The library's own contractual probe input: `VIPRSSYN` plus a view count
    /// and a primitive count, both `u32` little-endian. It needs no drawing
    /// file and no fixture, and it is the cheapest call that proves the runtime
    /// actually started.
    fn synthetic_document(views: u32, primitives: u32) -> Vec<u8> {
        let mut bytes = b"VIPRSSYN".to_vec();
        bytes.extend_from_slice(&views.to_le_bytes());
        bytes.extend_from_slice(&primitives.to_le_bytes());
        bytes
    }

    #[test]
    fn the_statically_linked_library_answers_a_real_call() {
        // Without `+whole-archive` this is where it aborts: the link was
        // clean, the symbols all resolved, and the runtime was never started.
        let limits = ffi::viprs_acad_limits_v1::default();
        let document = synthetic_document(3, 7);
        let mut handle = ptr::null_mut();
        // SAFETY: `document` is a live buffer I own for the whole call,
        // `limits` is a live struct of the type the callee expects, and
        // `handle` is a live out-pointer. The library owns `data` for the
        // duration of this call only.
        let code = unsafe {
            ffi::viprs_acad_open_memory(
                document.as_ptr(),
                document.len() as u64,
                &limits,
                &mut handle,
            )
        };
        assert_eq!(
            code,
            ffi::VIPRS_ACAD_OK,
            "the statically linked library refused the synthetic document with code {code}"
        );
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        // SAFETY: `handle` came back non-null from the open call above and has
        // not been closed, and `count` is a live `u32` the callee may write.
        let code = unsafe { ffi::viprs_acad_view_count(handle, &mut count) };
        // SAFETY: `handle` is a handle this library issued and I have not
        // closed it. Closed before the assertions so a failing one does not
        // leak the document.
        unsafe { ffi::viprs_acad_close(handle) };

        assert_eq!(
            code,
            ffi::VIPRS_ACAD_OK,
            "the view count call returned {code}"
        );
        assert_eq!(
            count, 3,
            "I asked the synthetic document for three views and the library counted {count}"
        );
    }

    #[test]
    fn the_test_binary_is_actually_statically_linked() {
        // The half the answer above cannot give me. A shared link that happens
        // to work returns the same 3.
        // Through a binding, because `assert!(cfg!(..))` is a constant
        // expression and clippy refuses those. What is decided at compile time
        // is the same either way.
        let elf_host = cfg!(target_os = "linux");
        assert!(
            elf_host,
            "the static archive is certified on Linux targets and on no other, so a static link \
             on this host is something nobody measured and this assertion has no instrument for it"
        );

        let exe = std::env::current_exe().expect("a test binary knows where it is");
        let dynamic = readelf_dynamic(&exe);

        assert!(
            dynamic.contains("NEEDED"),
            "readelf found no NEEDED entries at all in {}, which means it did not read the dynamic \
             section rather than that the section was clean. An instrument that reports nothing \
             reports a pass for every binary. It said:\n{dynamic}",
            exe.display()
        );
        assert!(
            !dynamic.contains("acadsharp_native"),
            "{} carries a dynamic dependency on the native library, so this binary is linked \
             against `lib/libacadsharp_native.so` rather than against the static archive beside \
             it. readelf said:\n{dynamic}",
            exe.display()
        );
        assert!(
            !dynamic.contains("RUNPATH") && !dynamic.contains("RPATH"),
            "{} carries a run path, and the static plan emits none. A run path on a static link is \
             what lets a binary that was supposed to be self-contained find the shared library and \
             run correctly on the build machine. readelf said:\n{dynamic}",
            exe.display()
        );
    }

    fn readelf_dynamic(exe: &PathBuf) -> String {
        let output = Command::new("readelf")
            .arg("-d")
            .arg(exe)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "I could not run readelf, so I cannot tell a static link from a shared one \
                     that works: {e}. This test refuses to skip, because a skipped check is the \
                     same colour as a passing one. Install binutils."
                )
            });
        assert!(
            output.status.success(),
            "readelf refused {}: {}",
            exe.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}
