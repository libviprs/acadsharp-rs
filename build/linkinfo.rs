//! `metadata/LINKINFO.json`, read by hand for the one field this half of the
//! build script needs.
//!
//! A hand reader rather than `serde`, because this is one flat array of short
//! strings and issue #3 is the PR that adds the dependency along with the rest
//! of the manifest. It refuses anything it does not understand instead of
//! guessing, so a manifest shape it was not written for stops the build rather
//! than quietly linking less than it should.
//!
//! It lives here rather than inside `build.rs` so `tests/build_manifest.rs` can
//! compile it and feed it a manifest, the same way `tests/abi_constants.rs`
//! compiles `build/sha256.rs` and feeds it the NIST vectors.
//!
//! # Everything in here ends up in a cargo directive, so everything in here is
//! checked
//!
//! The manifest arrives inside a downloaded tarball, and cargo's build-script
//! protocol is one directive per line of stdout. So a newline inside a string
//! this reader repeats is not a broken name, it is the end of one directive and
//! the start of another one the tarball chose. Proven end to end: a
//! `shared_system_libraries` entry of `"m\ncargo::rustc-link-arg=...`
//! put an arbitrary flag on the real link line, and a newline in
//! `ACADSHARP_NATIVE_DIR` split the `cargo::warning=` line the same way.
//!
//! CI pins the archive by sha256 and that is the real defence. A developer
//! pointing at an archive they fetched by hand has no such pin, so the reader
//! refuses rather than trusting the file.

use std::path::Path;

/// The bare library names one `"name": ["a", "b"]` field lists.
///
/// Every name comes back fit to put after `cargo::rustc-link-lib=`, or nothing
/// does: one bad entry refuses the whole manifest rather than being dropped,
/// because a link that silently leaves out a library it was told about fails
/// somewhere a long way from here.
pub fn system_libraries(json: &str, field: &str, manifest: &Path) -> Vec<String> {
    let names = string_array_field(json, field, manifest);
    for name in &names {
        check_library_name(name, field, manifest);
    }
    names
}

/// What a bare library name may be made of, and nothing else.
///
/// This is `[A-Za-z0-9_+.-]`, which covers every name any published archive has
/// ever listed (`m`, and that is the whole list today) and every plausible one:
/// `stdc++`, `pthread`, `gcc_s`, `c++abi`, `dl`, `rt`. It is deliberately an
/// allow list. A deny list of the characters I can think of today is a deny
/// list that meets a character I did not think of, and this string goes
/// straight into cargo's line-oriented protocol.
fn is_legal_in_a_library_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '.' | '-')
}

/// Refuses a library name that is not a bare library name.
fn check_library_name(name: &str, field: &str, manifest: &Path) {
    let Some(bad) = name.chars().find(|c| !is_legal_in_a_library_name(*c)) else {
        return;
    };
    panic!(
        "`{field}` in {} lists {name:?}, and {bad:?} is not something a bare library name may \
         contain. I emit these straight after `cargo::rustc-link-lib=`, one directive per line, \
         so a newline in there is not a broken name: it is a second directive that this manifest \
         chose and I would be repeating to cargo. Fix the manifest, or the archive it came out of.",
        manifest.display()
    );
}

/// Refuses an archive root with a control character in it.
///
/// Same reasoning as the names, one layer out. The root reaches cargo through
/// `rustc-link-search`, through the rpath and through the `cargo::warning=`
/// line that says no archive resolved, and a newline in `ACADSHARP_NATIVE_DIR`
/// splits any of them. Everything else a path may contain is fine: spaces,
/// unicode, a `:`, all of it survives a single directive line.
///
/// This is checked before the build script prints anything at all that carries
/// the root, including the warning, because the warning is one of the lines
/// the split lands in.
pub fn check_archive_root(root: &Path, manifest: &Path) {
    let shown = root.to_string_lossy();
    let Some(bad) = shown.chars().find(|c| c.is_control()) else {
        return;
    };
    panic!(
        "ACADSHARP_NATIVE_DIR resolves to {shown:?}, and {bad:?} in a path is a control \
         character I will not put into a cargo directive: one newline in there turns one \
         directive into two. The manifest I would have read is {:?}. Point the variable at a \
         directory whose name is a directory name.",
        manifest.to_string_lossy()
    );
}

/// Reads one `"name": ["a", "b"]` field out of a JSON document.
fn string_array_field(json: &str, field: &str, path: &Path) -> Vec<String> {
    let key = format!("\"{field}\"");
    let start = json.find(&key).unwrap_or_else(|| {
        panic!(
            "{} has no `{field}` field, so I cannot tell what to link",
            path.display()
        )
    });

    let rest = &json[start + key.len()..];
    let open = rest.find('[').unwrap_or_else(|| {
        panic!(
            "`{field}` in {} is not followed by an array",
            path.display()
        )
    });
    let colon = &rest[..open];
    assert!(
        colon.trim() == ":",
        "`{field}` in {} is not a plain `\"{field}\": [ ... ]` pair",
        path.display()
    );
    let close = rest[open..].find(']').unwrap_or_else(|| {
        panic!(
            "`{field}` in {} opens an array it never closes",
            path.display()
        )
    });
    let body = &rest[open + 1..open + close];

    let mut out = Vec::new();
    for item in body.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        assert!(
            !item.contains('\\'),
            "`{field}` in {} contains an escape sequence, and this reader does not decode those",
            path.display()
        );
        let unquoted = item
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or_else(|| {
                panic!(
                    "`{item}` in `{field}` of {} is not a quoted string",
                    path.display()
                )
            });
        assert!(
            !unquoted.is_empty(),
            "`{field}` in {} has an empty entry",
            path.display()
        );
        out.push(unquoted.to_string());
    }
    out
}
