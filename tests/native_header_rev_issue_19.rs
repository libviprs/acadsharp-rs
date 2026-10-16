//! Issue #19: `native/NATIVE_HEADER_REV` has to name a libviprs-dep commit
//! that `main` reaches, and the header at that commit has to be the header
//! vendored here.
//!
//! `docs/UPGRADING.md` says this pin "is checked by nothing". libviprs-dep's
//! `main` was then rewritten with identical trees, so the pin names a commit no
//! branch or tag reaches while the header bytes stay right. Nothing could have
//! noticed. These two tests are the check.
//!
//! Both need the network. Without it they fail and say so, unless
//! `ACADSHARP_OFFLINE=1`, which skips them out loud.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HEADER_IN_DEP: &str = "acadsharp/include/viprs_acadsharp.h";

fn dep_url() -> String {
    std::env::var("ACADSHARP_DEP_URL")
        .unwrap_or_else(|_| "https://github.com/libviprs/libviprs-dep.git".to_string())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn offline() -> bool {
    std::env::var("ACADSHARP_OFFLINE").as_deref() == Ok("1")
}

fn header_rev() -> String {
    let path = repo_root().join("native/NATIVE_HEADER_REV");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or_else(|| panic!("NATIVE_HEADER_REV has no payload line"))
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

/// A blob-less clone of libviprs-dep's `main` with the pinned commit fetched
/// into it. Blobs arrive lazily, only for what `git show` asks for.
fn dep_with_pin(name: &str, pin: &str) -> PathBuf {
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
        .arg(dep_url())
        .arg(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("could not run git clone: {e}"));
    assert!(
        out.status.success(),
        "could not clone {}'s main: {}\n\
         This check needs the network. Set ACADSHARP_OFFLINE=1 to skip it on purpose.",
        dep_url(),
        text(&out)
    );
    let fetched = git(&dir, &["fetch", "--quiet", "origin", pin]);
    assert!(
        fetched.status.success(),
        "NATIVE_HEADER_REV {pin} cannot be fetched from {} at all: {}",
        dep_url(),
        text(&fetched)
    );
    dir
}

#[test]
fn issue_19_native_header_rev_is_on_libviprs_dep_main() {
    if offline() {
        eprintln!("issue_19: ACADSHARP_OFFLINE=1, so I did NOT check NATIVE_HEADER_REV's reachability");
        return;
    }
    let pin = header_rev();
    let dep = dep_with_pin("issue_19_dep_reach", &pin);
    let ancestor = git(&dep, &["merge-base", "--is-ancestor", &pin, "refs/remotes/origin/main"]);
    if ancestor.status.success() {
        return;
    }
    let main = text(&git(&dep, &["rev-parse", "refs/remotes/origin/main"]));
    let tree = text(&git(&dep, &["rev-parse", &format!("{pin}^{{tree}}")]));
    let log = text(&git(&dep, &["log", "--format=%H %T", "refs/remotes/origin/main"]));
    let twin = log
        .lines()
        .find_map(|l| {
            let (c, t) = l.split_once(' ')?;
            (t == tree).then(|| c.to_string())
        })
        .unwrap_or_else(|| "no commit on main".to_string());
    panic!(
        "NATIVE_HEADER_REV {pin} is not on libviprs-dep's main (main is at {main}), so the \
         provenance it records points at a commit no branch or tag reaches. \
         The commit on main with the same tree ({tree}) is: {twin}."
    );
}

#[test]
fn issue_19_the_header_at_native_header_rev_is_the_vendored_header() {
    if offline() {
        eprintln!("issue_19: ACADSHARP_OFFLINE=1, so I did NOT compare the vendored header with NATIVE_HEADER_REV's");
        return;
    }
    let pin = header_rev();
    let dep = dep_with_pin("issue_19_dep_bytes", &pin);
    let shown = git(&dep, &["show", &format!("{pin}:{HEADER_IN_DEP}")]);
    assert!(
        shown.status.success(),
        "libviprs-dep at {pin} has no {HEADER_IN_DEP}: {}",
        text(&shown)
    );
    let vendored_path = repo_root().join("native/viprs_acadsharp.h");
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
