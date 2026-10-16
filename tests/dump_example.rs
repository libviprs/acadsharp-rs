//! The `dump` example is a measuring instrument for the AOT parity run, so it
//! must never answer "nothing to compare" with success.
//!
//! In a build with no native archive, `Decoder::new` answers `Error::Unlinked`
//! and the example has to turn that into a non-zero exit with the message. A
//! zero here would let a parity run over an unlinked build diff an empty dump
//! against every recording and report each one as the first record missing,
//! or, worse, against an empty expectation as identical.
//!
//! The example is driven as a program, from the directory cargo puts the
//! examples in, with `ACADSHARP_NATIVE_DIR` removed from its environment.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn dump_example() -> PathBuf {
    // <target>/<profile>/deps/<this test> -> <target>/<profile>/examples/dump
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("the test binary sits in <profile>/deps");
    let path = profile_dir.join("examples").join("dump");
    assert!(
        path.exists(),
        "the dump example is not built at {}; `cargo test` builds examples, so run it that way",
        path.display()
    );
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(dump_example())
        .args(args)
        .env_remove("ACADSHARP_NATIVE_DIR")
        .output()
        .expect("run the dump example")
}

fn all(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[cfg(not(acadsharp_linked))]
#[test]
fn an_unlinked_build_exits_non_zero_and_says_why() {
    let out = run(&["/nonexistent.dwg"]);
    assert_ne!(
        out.status.code(),
        Some(0),
        "the example exited 0 with no native library behind it:\n{}",
        all(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no native library"),
        "the refusal does not say the build is unlinked:\n{}",
        all(&out)
    );
    assert!(
        out.stdout.is_empty(),
        "an unlinked build printed a dump on stdout:\n{}",
        all(&out)
    );
}

#[test]
fn no_arguments_is_a_usage_error_and_not_a_success() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(64), "{}", all(&out));
    assert!(all(&out).contains("usage:"), "{}", all(&out));
}

#[test]
fn a_view_index_that_is_not_a_number_is_a_usage_error() {
    let out = run(&["x.dwg", "first"]);
    assert_eq!(out.status.code(), Some(64), "{}", all(&out));
}
