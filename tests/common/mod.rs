//! Small helpers the header-reading tests share.
//!
//! Every one of them reads `native/viprs_acadsharp.h`, the vendored copy of the
//! frozen header, because that file is the contract. A test that hard-codes
//! what the header says is a test that keeps agreeing with itself after the
//! header moves, which is the one direction that does damage.

#![allow(dead_code)]

use std::path::PathBuf;

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

/// Drops every `/* ... */` comment, including the multi-line ones, and leaves
/// everything else alone.
///
/// The header puts a comment on most struct fields and several of them wrap
/// over two lines, so a line-oriented filter gets the second half of one
/// wrong. This is a two-state machine instead, which cannot.
pub fn strip_block_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut in_comment = false;
    while i < chars.len() {
        if in_comment {
            if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                in_comment = false;
                i += 2;
            } else {
                // Keep newlines so line numbers and blank-line structure survive.
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
        } else if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            in_comment = true;
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    assert!(
        !in_comment,
        "the vendored header has an unterminated block comment, so this parser cannot trust anything it found"
    );
    out
}

/// Every `#define NAME VALUE` in `source`, in the order they appear, with the
/// `u` suffix on the value already dropped.
///
/// Only integer defines are recognised. A define whose value is not a decimal
/// number is a panic rather than a skip, because a silently skipped line is
/// how a parser ends up reporting an empty set and passing.
pub fn integer_defines(source: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#define ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(value) = parts.next() else {
            // `#define VIPRS_ACADSHARP_H` and friends: an include guard, not a
            // constant. Nothing to compare, so there is nothing to record.
            continue;
        };
        let digits = value.trim_end_matches(['u', 'U']);
        let parsed = digits.parse::<u64>().unwrap_or_else(|e| {
            panic!("`#define {name} {value}` is not a decimal integer and this parser only handles those: {e}")
        });
        out.push((name.to_string(), parsed));
    }
    out
}
