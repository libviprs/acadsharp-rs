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
