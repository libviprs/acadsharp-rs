//! One test whose whole job is to refuse a silent skip.
//!
//! Everything that touches the native library is `#[cfg(acadsharp_linked)]`,
//! and that cfg only appears when `build.rs` found an archive. A job that was
//! meant to fetch the archive and produced nothing therefore runs zero native
//! tests and goes green, and a skipped lane is the same colour as a passing
//! one. So: if the environment says an archive is there, the cfg has to be
//! there too, and if it is not, this fails and says so.

#[test]
fn an_archive_in_the_environment_means_the_native_lane_actually_linked() {
    let Some(dir) = std::env::var_os("ACADSHARP_NATIVE_DIR") else {
        // No archive was offered, so nothing was skipped. The crate is
        // supposed to build and test without one, which is what the rest of
        // this run is proving.
        eprintln!("ACADSHARP_NATIVE_DIR is unset, so the native lane is off on purpose");
        return;
    };

    assert!(
        cfg!(acadsharp_linked),
        "ACADSHARP_NATIVE_DIR is set to {dir:?} but `cfg(acadsharp_linked)` is not, so every native test in this \
         run was compiled out and the run went green without calling the library once. Check the build script's \
         warnings: it says which of `lib/` and `metadata/LINKINFO.json` it could not find under that directory."
    );
}
