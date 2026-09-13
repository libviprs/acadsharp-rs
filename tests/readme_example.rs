//! The README's API example is the crate's doctest, character for character.
//!
//! An example in a README nothing compiles is an example that stops working
//! and says nothing about it. The one in `src/lib.rs` is a real doctest, so
//! this only has to check that the README is showing the same text rather
//! than a plausible relative of it.

use std::path::Path;

/// Pulls the one ```rust fenced block out of a markdown-ish text.
fn fenced_rust(text: &str, what: &str) -> String {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let fence_body = trimmed.strip_prefix("```");
        match (&mut current, fence_body) {
            (None, Some(info)) if info.trim() == "rust" => current = Some(Vec::new()),
            (Some(lines), Some(_)) => {
                blocks.push(lines.join("\n"));
                current = None;
            }
            (Some(lines), None) => lines.push(line),
            _ => {}
        }
    }
    assert!(current.is_none(), "{what} has an unclosed ``` fence");
    assert_eq!(
        blocks.len(),
        1,
        "{what} should carry exactly one ```rust block, and it has {}",
        blocks.len()
    );
    blocks.remove(0)
}

#[test]
fn the_readme_example_is_the_doctest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("the README");
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("src/lib.rs");

    // The crate docs are `//!` lines, so strip the marker before comparing.
    let doc: String = lib
        .lines()
        .take_while(|line| line.starts_with("//!") || line.trim().is_empty())
        .map(|line| line.strip_prefix("//! ").or(line.strip_prefix("//!")).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let from_readme = fenced_rust(&readme, "README.md");
    let from_doc = fenced_rust(&doc, "the crate documentation");

    assert_eq!(
        from_readme, from_doc,
        "the README example and the crate's doctest have drifted apart, and only one of \
         them is compiled"
    );
    assert!(
        from_readme.contains("Decoder::new"),
        "the example should actually open something"
    );
    assert!(
        !from_readme.contains("# "),
        "the README cannot hide doctest lines, so the example must not need any"
    );
}
