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

use std::path::Path;

/// The bare library names one `"name": ["a", "b"]` field lists.
pub fn system_libraries(json: &str, field: &str, manifest: &Path) -> Vec<String> {
    string_array_field(json, field, manifest)
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
