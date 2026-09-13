//! Reading the vendored header: dropping its block comments, and pulling an
//! integer `#define` out of what is left.
//!
//! This lives in `build/` beside `sha256.rs` because both sides of the
//! generated constants need it. `build.rs` writes `EXPECTED_ABI_VERSION` and
//! friends out of the header's bytes, and `tests/` re-derives the same numbers
//! to check them, so a parser only one side used would be checked by a copy of
//! itself.

/// Drops every `/* ... */` comment, including the multi-line ones, and leaves
/// everything else alone.
///
/// The header puts a comment on most struct fields and several of them wrap
/// over two lines, so a line-oriented filter gets the second half of one
/// wrong. This is a two-state machine instead, which cannot.
///
/// Newlines inside a comment survive, so a line number in the output is the
/// same line number as in the input.
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
        "the source I am parsing has an unterminated block comment, so this parser cannot trust anything it found"
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

/// The value of `#define NAME <decimal>` in `source`, where `what` names the
/// file it came from so a refusal says which one to go and look at.
pub fn integer_define(source: &str, name: &str, what: &str) -> u64 {
    for (found, value) in integer_defines(source) {
        if found == name {
            return value;
        }
    }
    panic!("{what} has no `#define {name}`, so I have nothing to read it from")
}
