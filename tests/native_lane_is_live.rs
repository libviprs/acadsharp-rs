//! One test whose whole job is to refuse a silent skip.
//!
//! Everything that touches the native library is `#[cfg(acadsharp_linked)]`,
//! and that cfg only appears when `build.rs` found an archive. A job that was
//! meant to fetch the archive and produced nothing therefore runs zero native
//! tests and goes green, and a skipped lane is the same colour as a passing
//! one. So: if the environment says an archive is there, the cfg has to be
//! there too, and if it is not, this fails and says so.
//!
//! That arm alone has a hole, and it is the likelier failure. It fires only
//! when `ACADSHARP_NATIVE_DIR` is **set**, so it cannot fire when the variable
//! is never exported at all: one edited `>> "$GITHUB_ENV"` line, one step
//! rename, one refactor, and the job runs zero native tests and stays green
//! with the guard satisfied. So the job says what it wants instead, through
//! `ACADSHARP_REQUIRE_NATIVE`, set as a job-level `env:` where a step edit
//! cannot lose it.

#[test]
fn an_archive_in_the_environment_means_the_native_lane_actually_linked() {
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        // No archive was offered, so nothing was skipped. The crate is
        // supposed to build and test without one, which is what the rest of
        // this run is proving.
        eprintln!("ACADSHARP_NATIVE_DIR is unset, so the native lane is off on purpose");
        return;
    };

    // Through a binding rather than inline, because `assert!(cfg!(..))` is a
    // constant expression and clippy refuses those. The value is decided at
    // compile time either way; what is decided at run time is whether the
    // environment claimed an archive.
    let linked = cfg!(acadsharp_linked);
    assert!(
        linked,
        "ACADSHARP_NATIVE_DIR is set to {dir:?} but `cfg(acadsharp_linked)` is not, so every native test in this \
         run was compiled out and the run went green without calling the library once. Check the build script's \
         warnings: it says which of `lib/` and `metadata/LINKINFO.json` it could not find under that directory."
    );
}

/// Whether this run is one that is supposed to call the library.
///
/// Set as a job-level `env:` in CI's `Test` job. I treat an empty value and a
/// `0` as unset so nobody has to remember which spelling turns it off.
fn native_lane_required() -> bool {
    match std::env::var("ACADSHARP_REQUIRE_NATIVE") {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

#[test]
fn a_job_that_says_it_requires_the_native_lane_gets_the_native_lane() {
    if !native_lane_required() {
        eprintln!(
            "ACADSHARP_REQUIRE_NATIVE is unset, so this is a developer run or one of the three CI \
             jobs that never link, and nothing is being skipped"
        );
        return;
    }

    let linked = cfg!(acadsharp_linked);
    assert!(
        linked,
        "ACADSHARP_REQUIRE_NATIVE is set, so this job exists to call the native library, and \
         `cfg(acadsharp_linked)` is absent: every native test compiled out and the run is about to \
         go green without one call to the library. ACADSHARP_NATIVE_DIR is {:?}, which is the \
         first thing to look at: unset means the fetch step never exported it, and set means the \
         build script did not find `lib/` and `metadata/LINKINFO.json` under it.",
        std::env::var_os("ACADSHARP_NATIVE_DIR")
    );
}
