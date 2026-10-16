//! Issue #19: `native/NATIVE_HEADER_REV` has to name a libviprs-dep commit
//! that `main` reaches, and the header at that commit has to be the header
//! vendored here.
//!
//! `docs/UPGRADING.md` used to say this pin "is checked by nothing".
//! libviprs-dep's `main` was rewritten with identical trees, so the pin named a
//! commit no branch or tag reached while the header bytes stayed right, and
//! nothing could have noticed. This is the check.
//!
//! One clone serves both assertions. The ancestry question goes to
//! `tools/suite_pin_check.sh reachable` with the pin file named, called with
//! `GITHUB_REF_NAME=main`: this pin never names a PR head, so the leniency
//! `SUITE_REV` has on a PR ref does not apply and only an ancestor passes.
//!
//! Needs the network. Without it the test fails and says so, unless
//! `ACADSHARP_OFFLINE=1`, which skips it out loud.
#![cfg(unix)]

mod common;

const HEADER_IN_DEP: &str = "acadsharp/include/viprs_acadsharp.h";
const PIN_FILE: &str = "native/NATIVE_HEADER_REV";

fn dep_url() -> String {
    std::env::var("ACADSHARP_DEP_URL")
        .unwrap_or_else(|_| "https://github.com/libviprs/libviprs-dep.git".to_string())
}

#[test]
fn issue_19_native_header_rev_is_on_libviprs_dep_main_and_names_the_vendored_header() {
    if common::offline() {
        eprintln!(
            "issue_19: ACADSHARP_OFFLINE=1, so I did NOT check NATIVE_HEADER_REV's reachability \
             or compare the vendored header with its"
        );
        return;
    }
    let pin = common::pin_payload(PIN_FILE);
    let dep = common::clone_main_with_pin(&dep_url(), "issue_19_dep", &pin);

    let out = common::reachable(&dep, &pin, "main", Some(PIN_FILE));
    assert_eq!(
        out.status.code(),
        Some(0),
        "NATIVE_HEADER_REV {pin} is not on {}'s main, so the provenance it records points at a \
         commit no branch or tag reaches:\n{}",
        dep_url(),
        common::text(&out)
    );

    let shown = common::git(&dep, &["show", &format!("{pin}:{HEADER_IN_DEP}")]);
    assert!(
        shown.status.success(),
        "libviprs-dep at {pin} has no {HEADER_IN_DEP}: {}",
        common::text(&shown)
    );
    let vendored_path = common::header_path();
    let vendored = std::fs::read(&vendored_path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", vendored_path.display()));
    assert!(
        shown.stdout == vendored,
        "native/viprs_acadsharp.h is not byte for byte {HEADER_IN_DEP} at NATIVE_HEADER_REV {pin} \
         ({} bytes here, {} there)",
        vendored.len(),
        shown.stdout.len()
    );
}
