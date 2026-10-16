//! Issue #17: `SUITE_REV` has to name a commit the suite's `main` reaches.
//!
//! `tests/suite_pin_check.rs` proves the pin's shape. This proves the pin is
//! real: it fetches acadsharp-rs-tests' `main` without blobs and asks git
//! whether `SUITE_REV` is an ancestor of it.
//!
//! The case it exists for is a rewritten history. acadsharp-rs-tests' `main`
//! was rewritten with byte-identical trees after `SUITE_REV` was written, so
//! the pinned commit is still fetchable by sha (GitHub keeps unreferenced
//! objects until it collects them) while no branch reaches it. The Suite job's
//! fetch step passes in that state, which is why a fetch is not a reachability
//! check.
//!
//! This needs the network. Without it the test fails and says so, unless
//! `ACADSHARP_OFFLINE=1`, which skips it out loud. A check that quietly passes
//! when it could not look is the thing this file is about.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Where the suite lives. Overridable so a fork of the pair can point the
/// check at itself.
fn suite_url() -> String {
    std::env::var("ACADSHARP_SUITE_URL")
        .unwrap_or_else(|_| "https://github.com/libviprs/acadsharp-rs-tests.git".to_string())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn offline() -> bool {
    std::env::var("ACADSHARP_OFFLINE").as_deref() == Ok("1")
}

/// The first line of `SUITE_REV` that is neither blank nor a comment, the same
/// rule the workflow's awk and `suite_pin_check.rs` use.
fn suite_rev() -> String {
    let path = repo_root().join("SUITE_REV");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or_else(|| panic!("SUITE_REV has no payload line"))
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("could not run git {args:?}: {e}"))
}

fn text(out: &Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s.trim().to_string()
}

/// A blob-less clone of the suite's `main`, in a directory of its own under
/// cargo's per-target temp dir. Trees and commits come down, file contents do
/// not, so this stays small.
fn clone_suite_main(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "--filter=blob:none",
            "--no-checkout",
            "--single-branch",
            "--branch",
            "main",
        ])
        .arg(suite_url())
        .arg(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("could not run git clone: {e}"));
    assert!(
        out.status.success(),
        "could not clone {}'s main to check SUITE_REV against it: {}\n\
         This check needs the network. Set ACADSHARP_OFFLINE=1 to skip it on purpose.",
        suite_url(),
        text(&out)
    );
    dir
}

#[test]
fn issue_17_suite_rev_is_on_the_suite_main_branch() {
    if offline() {
        eprintln!(
            "issue_17: ACADSHARP_OFFLINE=1, so I did NOT check that SUITE_REV is on the suite's main"
        );
        return;
    }

    let pin = suite_rev();
    let repo = clone_suite_main("issue_17_suite_main");
    let main = text(&git(&repo, &["rev-parse", "refs/remotes/origin/main"]));

    // Fetching the pin is allowed to succeed even when no branch reaches it;
    // that is exactly the state this test is about. Failing to fetch it at all
    // is the later, louder version of the same bug.
    let fetched = git(&repo, &["fetch", "--quiet", "origin", &pin]);
    assert!(
        fetched.status.success(),
        "SUITE_REV {pin} cannot be fetched from {} at all: {}",
        suite_url(),
        text(&fetched)
    );

    let ancestor = git(&repo, &["merge-base", "--is-ancestor", &pin, "refs/remotes/origin/main"]);
    if ancestor.status.success() {
        return;
    }

    // Not an ancestor. Say whether main carries the same tree under another
    // sha, because that is what a rewrite looks like and it tells the reader
    // which commit to move the pin to.
    let tree = text(&git(&repo, &["rev-parse", &format!("{pin}^{{tree}}")]));
    let log = text(&git(&repo, &["log", "--format=%H %T", "refs/remotes/origin/main"]));
    let twin = log
        .lines()
        .find_map(|l| {
            let (c, t) = l.split_once(' ')?;
            (t == tree).then(|| c.to_string())
        })
        .unwrap_or_else(|| "no commit on main".to_string());

    panic!(
        "SUITE_REV {pin} is not on acadsharp-rs-tests' main (main is at {main}). \
         GitHub still serves it by sha, which is why the Suite job's fetch step passes, \
         but no branch reaches it, so it disappears whenever GitHub collects unreferenced objects. \
         The commit on main with the same tree ({tree}) is: {twin}."
    );
}
