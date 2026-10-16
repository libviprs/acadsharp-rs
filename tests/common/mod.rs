//! Small helpers the header-reading tests share.
//!
//! Every one of them reads `native/viprs_acadsharp.h`, the vendored copy of the
//! frozen header, because that file is the contract. A test that hard-codes
//! what the header says is a test that keeps agreeing with itself after the
//! header moves, which is the one direction that does damage.
//!
//! The parsing itself lives in `build/header.rs` and is compiled in here, so
//! the tests and `build.rs` read the header through exactly the same code. A
//! second copy over here is a copy that can agree with a wrong answer, and it
//! did: see that module's own documentation.

#![allow(dead_code)]

use std::path::PathBuf;

#[path = "../../build/header.rs"]
pub mod header;

/// Where the vendored header lives, resolved from the manifest directory so the
/// test does not care what the working directory is.
pub fn header_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/viprs_acadsharp.h")
}

/// The vendored header's bytes as text.
pub fn header_text() -> String {
    let path = header_path();
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "I could not read the vendored header at {}: {e}",
            path.display()
        )
    })
}

/// What a refusal from the header parser should call the file, so a message
/// says which one to go and look at.
pub fn header_name() -> String {
    "native/viprs_acadsharp.h".to_string()
}

/// Runs `f`, expects it to refuse, and hands back what it said.
///
/// The refusals these parsers hand out are panics, because a build script has
/// nowhere else to put a refusal. A `#[should_panic]` attribute would check
/// the message by substring and say nothing about which part matched, so this
/// returns the message instead and lets the test assert on it properly. The
/// panic hook goes quiet for the duration, or a passing test prints a
/// backtrace and reads like a failing one.
pub fn refusal(what: &str, f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(f);
    std::panic::set_hook(previous);

    let payload = match outcome {
        Ok(()) => panic!("I expected {what} to be refused, and it went through without a word"),
        Err(payload) => payload,
    };
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else {
        panic!("{what} was refused and the refusal carried no message at all")
    }
}

// ---------------------------------------------------------------------------
// Pins that name a commit in another repository
// ---------------------------------------------------------------------------
//
// `SUITE_REV` and `native/NATIVE_HEADER_REV` both name a commit that has to be
// on another repository's `main`. The ancestry rule lives once, in
// `tools/suite_pin_check.sh reachable`; these helpers only build the clone it
// reads and call it, so the job, the Docker path and `cargo test` all ask the
// same question.

use std::path::Path;
use std::process::{Command, Output};

/// True when `ACADSHARP_OFFLINE=1`, the one way to skip a network check, and
/// only out loud.
pub fn offline() -> bool {
    std::env::var("ACADSHARP_OFFLINE").as_deref() == Ok("1")
}

/// The first line of `file` (relative to the manifest directory) that is
/// neither blank nor a comment: the rule the workflow's awk uses too.
pub fn pin_payload(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or_else(|| panic!("{file} has no payload line"))
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// stdout then stderr, trimmed.
pub fn text(out: &Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s.trim().to_string()
}

/// Runs git in `dir` with prompts off.
pub fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("could not run git {args:?}: {e}"))
}

/// A blob-less clone of `url`'s `main` under cargo's per-target temp dir, with
/// `pin` fetched into it the same way and with no `--depth`, because a
/// shallow pin has no parents on disk and `merge-base` could not tell an open
/// PR head from an unrelated history. Returns the clone's work tree; its git
/// dir is `<that>/.git`. Panics, naming the network, when it cannot clone or
/// fetch.
pub fn clone_main_with_pin(url: &str, name: &str, pin: &str) -> PathBuf {
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
        .arg(url)
        .arg(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("could not run git clone: {e}"));
    assert!(
        out.status.success(),
        "could not clone {url}'s main to check a pin against it: {}\n\
         This check needs the network. Set ACADSHARP_OFFLINE=1 to skip it on purpose.",
        text(&out)
    );
    let fetched = git(
        &dir,
        &["fetch", "--quiet", "--filter=blob:none", "origin", pin],
    );
    assert!(
        fetched.status.success(),
        "{pin} cannot be fetched from {url} at all: {}",
        text(&fetched)
    );
    dir
}

/// `tools/suite_pin_check.sh reachable <clone>/.git <pin> [<pin-file>]`.
pub fn reachable(clone: &Path, pin: &str, ref_name: &str, pin_file: Option<&str>) -> Output {
    let mut cmd =
        Command::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/suite_pin_check.sh"));
    cmd.arg("reachable").arg(clone.join(".git")).arg(pin);
    if let Some(f) = pin_file {
        cmd.arg(f);
    }
    cmd.env("GITHUB_REF_NAME", ref_name)
        .output()
        .expect("run tools/suite_pin_check.sh")
}
