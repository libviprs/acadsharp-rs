//! Issue #17: `SUITE_REV` has to name a commit the suite's `main` reaches.
//!
//! `tests/suite_pin_check.rs` proves the pin's shape. This proves the pin is
//! real: it clones acadsharp-rs-tests' `main` without blobs, fetches the pin
//! into the clone and asks `tools/suite_pin_check.sh reachable` whether `main`
//! reaches it. The ancestry rule lives in that script once, so this test, the
//! job and the Docker path cannot disagree.
//!
//! The case it exists for is a rewritten history. The suite's `main` was
//! rewritten with byte-identical trees after the pin was written, so the old
//! commit stayed fetchable by sha (GitHub keeps unreferenced objects until it
//! collects them) while no branch reached it, and the job's fetch step passed.
//!
//! This needs the network. Without it the test fails and says so, unless
//! `ACADSHARP_OFFLINE=1`, which skips it out loud.
#![cfg(unix)]

mod common;

fn suite_url() -> String {
    std::env::var("ACADSHARP_SUITE_URL")
        .unwrap_or_else(|_| "https://github.com/libviprs/acadsharp-rs-tests.git".to_string())
}

#[test]
fn issue_17_suite_rev_is_on_the_suite_main_branch() {
    if common::offline() {
        eprintln!(
            "issue_17: ACADSHARP_OFFLINE=1, so I did NOT check that SUITE_REV is on the suite's main"
        );
        return;
    }
    let pin = common::pin_payload("SUITE_REV");
    let clone = common::clone_main_with_pin(&suite_url(), "issue_17_suite_main", &pin);
    // `main` on purpose: only an ancestor passes, so an open suite PR head
    // cannot hide here the way it may on a PR ref.
    let out = common::reachable(&clone, &pin, "main", None);
    assert_eq!(
        out.status.code(),
        Some(0),
        "SUITE_REV {pin} is not on {}'s main. GitHub may still serve it by sha, which is why \
         the Suite job's fetch step can pass, but no branch reaches it:\n{}",
        suite_url(),
        common::text(&out)
    );
}
