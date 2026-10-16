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
use std::sync::Once;

/// The example as a plain program. `cargo test` builds an example that sets
/// `test = true` only as a test harness, so this builds the real one, into the
/// same target directory and profile this test is running from, once.
fn dump_example() -> PathBuf {
    static BUILT: Once = Once::new();
    // <target>/<profile>/deps/<this test> -> <target>/<profile>
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("the test binary sits in <profile>/deps")
        .to_path_buf();
    let target_dir = profile_dir.parent().expect("a target directory");
    BUILT.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(["build", "--quiet", "--example", "dump", "--manifest-path"])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        if profile_dir.file_name().is_some_and(|n| n == "release") {
            cmd.arg("--release");
        }
        let out = cmd.output().expect("run cargo build --example dump");
        assert!(
            out.status.success(),
            "could not build the dump example:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    });
    let path = profile_dir.join("examples").join("dump");
    assert!(path.exists(), "no dump example at {}", path.display());
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
