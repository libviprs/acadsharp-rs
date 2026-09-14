//! `COMPAT.toml`: what this crate declares it is compatible with, and the
//! checks that make the declaration load bearing.
//!
//! Five keys, and none of the five is a fact this crate owns. `abi_version`
//! and `wire_version` belong to the vendored header, `abi_header_sha256`
//! belongs to that header's bytes, and the two artifact fields describe
//! archives `libviprs-dep` publishes. So the file is a declaration that has to
//! agree with something else, and every value in it is compared against the
//! thing it describes before the build is allowed to continue.
//!
//! # Why the header still wins
//!
//! The issue asks for the declaration and the enforcement to be the same
//! bytes, and reads that as `build.rs` taking `EXPECTED_ABI_VERSION` out of
//! this file. I did it the other way round: the constants stay derived from
//! the header's `#define`s, and a `COMPAT.toml` that disagrees with the header
//! **stops the build**.
//!
//! Both shapes give you one number. They differ in which direction a mistake
//! travels. Read the number out of the TOML and the TOML becomes a second hand
//! typed source of truth: somebody bumps the header, forgets this file, and
//! the crate cheerfully compiles against a library it now believes is version
//! 2 while the header says 3. Derive from the header and refuse a
//! disagreement, and the same mistake is a build failure naming both files.
//! The declaration is still load bearing, because a wrong value in it stops
//! everything, and it is still one source of truth, because the bytes of the
//! header are the ones that decide.
//!
//! # What checks what
//!
//! | key | checked against | where |
//! | --- | --- | --- |
//! | `abi_version` | `#define VIPRS_ACAD_ABI_VERSION` | [`Compat::check_against_header`] |
//! | `wire_version` | `#define VIPRS_ACAD_WIRE_VERSION` | [`Compat::check_against_header`] |
//! | `abi_header_sha256` | the header's recomputed digest | [`Compat::check_against_header`] |
//! | `native_artifact_versions` | `artifact_version` in `metadata/LINKINFO.json` | [`Compat::check_archive`] |
//! | `acadsharp_versions` | `acadsharp_version` in `metadata/LINKINFO.json` | [`Compat::check_archive`] |
//!
//! `tests/compat.rs` compiles this file into a real test target and runs every
//! one of those checks, in both directions, against the real header and the
//! real published manifest. A build script's own `#[cfg(test)]` module is
//! compiled by nothing, and a test that never runs is the same colour as one
//! that passed.

use std::path::Path;

/// The declaration's filename, relative to the manifest directory.
///
/// One constant rather than a literal in five refusal messages, because a
/// refusal that names the wrong file sends somebody to the wrong file.
pub const COMPAT_FILE: &str = "COMPAT.toml";

/// Every key the declaration may carry, in the order they appear in the file
/// and in the README table.
///
/// This is an allow list. A key this parser has never heard of is a refusal
/// rather than something to skip: a key nobody reads is exactly what a
/// declaration is not allowed to grow, and a typo in a key that mattered would
/// otherwise read as a missing optional.
pub const KEYS: [&str; 5] = [
    "abi_version",
    "wire_version",
    "abi_header_sha256",
    "native_artifact_versions",
    "acadsharp_versions",
];

/// What each key is checked against, for the generated README table.
///
/// It lives beside the checks rather than in the README, so a row cannot claim
/// a check that does not exist.
const CHECKED_AGAINST: [&str; 5] = [
    "`#define VIPRS_ACAD_ABI_VERSION` in `native/viprs_acadsharp.h`, every build",
    "`#define VIPRS_ACAD_WIRE_VERSION` in `native/viprs_acadsharp.h`, every build",
    "the sha256 of `native/viprs_acadsharp.h`, recomputed every build",
    "`artifact_version` in the archive's `metadata/LINKINFO.json`, when one resolves",
    "`acadsharp_version` in the archive's `metadata/LINKINFO.json`, when one resolves",
];

/// Where the generated table starts in `README.md`.
pub const README_TABLE_BEGIN: &str = "<!-- BEGIN generated from COMPAT.toml -->";
/// Where it ends.
pub const README_TABLE_END: &str = "<!-- END generated from COMPAT.toml -->";

/// The five declared values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compat {
    /// The ABI version the vendored header has to declare.
    pub abi_version: u32,
    /// The batch protocol version the vendored header has to declare.
    pub wire_version: u32,
    /// The sha256 of the vendored header, 64 lowercase hex characters.
    pub abi_header_sha256: String,
    /// A glob over `LINKINFO.artifact_version`. See [`glob_matches`].
    pub native_artifact_versions: String,
    /// Every upstream ACadSharp version this crate is known to work against.
    pub acadsharp_versions: Vec<String>,
}

impl Compat {
    /// Reads and parses the declaration at `path`.
    pub fn read(path: &Path) -> Compat {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!(
                "I could not read the compatibility declaration at {}: {e}. It is committed and \
                 the build reads it, so this means the checkout is incomplete rather than that \
                 something needs fetching.",
                path.display()
            )
        });
        Compat::parse(&text, &path.display().to_string())
    }

    /// Parses the declaration, where `what` names the file a refusal should
    /// send somebody to.
    pub fn parse(text: &str, what: &str) -> Compat {
        let mut abi_version = None;
        let mut wire_version = None;
        let mut abi_header_sha256 = None;
        let mut native_artifact_versions = None;
        let mut acadsharp_versions = None;

        for (index, raw) in text.lines().enumerate() {
            let line = strip_comment(raw, index + 1, what).trim();
            if line.is_empty() {
                continue;
            }
            assert!(
                !line.starts_with('['),
                "{what} line {}: this reader handles one flat table of five keys and nothing \
                 else, so `{line}` is a shape it cannot describe rather than one it should guess \
                 at.",
                index + 1
            );
            let Some((key, value)) = line.split_once('=') else {
                panic!(
                    "{what} line {}: `{line}` is not a `key = value` pair.",
                    index + 1
                )
            };
            let key = key.trim();
            let value = value.trim();
            let at = Where {
                what,
                line: index + 1,
            };

            match key {
                "abi_version" => set(&mut abi_version, key, at, integer(value, key, at)),
                "wire_version" => set(&mut wire_version, key, at, integer(value, key, at)),
                "abi_header_sha256" => {
                    set(&mut abi_header_sha256, key, at, digest(value, key, at));
                }
                "native_artifact_versions" => {
                    set(
                        &mut native_artifact_versions,
                        key,
                        at,
                        string(value, key, at),
                    );
                }
                "acadsharp_versions" => {
                    set(
                        &mut acadsharp_versions,
                        key,
                        at,
                        string_array(value, key, at),
                    );
                }
                _ => panic!(
                    "{what} line {}: `{key}` is not one of the keys this declaration may carry, \
                     which are {KEYS:?}. A key nothing reads is decoration, so I refuse it rather \
                     than skip it.",
                    index + 1
                ),
            }
        }

        let missing: Vec<&str> = KEYS
            .iter()
            .copied()
            .filter(|key| match *key {
                "abi_version" => abi_version.is_none(),
                "wire_version" => wire_version.is_none(),
                "abi_header_sha256" => abi_header_sha256.is_none(),
                "native_artifact_versions" => native_artifact_versions.is_none(),
                _ => acadsharp_versions.is_none(),
            })
            .collect();
        assert!(
            missing.is_empty(),
            "{what} does not set {missing:?}. Every key is required: an absent one would be a \
             check that quietly does not happen, which is the failure this whole file exists to \
             prevent."
        );

        let compat = Compat {
            abi_version: abi_version.expect("checked above"),
            wire_version: wire_version.expect("checked above"),
            abi_header_sha256: abi_header_sha256.expect("checked above"),
            native_artifact_versions: native_artifact_versions.expect("checked above"),
            acadsharp_versions: acadsharp_versions.expect("checked above"),
        };
        assert!(
            !compat.native_artifact_versions.is_empty(),
            "{what} sets `native_artifact_versions` to an empty glob, which matches no archive \
             at all and would refuse every build that has one."
        );
        assert!(
            !compat.acadsharp_versions.is_empty(),
            "{what} sets `acadsharp_versions` to an empty list, which refuses every archive \
             there is."
        );
        compat
    }

    /// Refuses to build when the declaration and the vendored header describe
    /// different headers.
    ///
    /// The three numbers handed in here all come out of the header's own bytes
    /// in `build.rs`, so this is the declaration being checked against the
    /// contract and never the other way round. See the module documentation
    /// for why that direction.
    pub fn check_against_header(
        &self,
        header_abi_version: u32,
        header_wire_version: u32,
        header_sha256: &str,
        header_file: &str,
        compat_file: &str,
    ) {
        assert!(
            self.abi_version == header_abi_version,
            "{compat_file} declares `abi_version = {}` and {header_file} defines \
             `VIPRS_ACAD_ABI_VERSION` as {header_abi_version}. The header is the contract, so it \
             is {compat_file} that is wrong: either the header moved and the declaration was left \
             behind, or somebody typed a number into the declaration. Fix {compat_file}; do not \
             touch the header to make this go away.",
            self.abi_version
        );
        assert!(
            self.wire_version == header_wire_version,
            "{compat_file} declares `wire_version = {}` and {header_file} defines \
             `VIPRS_ACAD_WIRE_VERSION` as {header_wire_version}. These two move independently of \
             each other and of `abi_version`, so a wire version copied from an ABI bump is \
             exactly the mistake this catches. Fix {compat_file}.",
            self.wire_version
        );
        assert!(
            self.abi_header_sha256 == header_sha256,
            "{compat_file} declares `abi_header_sha256 = \"{}\"` and {header_file} hashes to \
             {header_sha256}. Either the header was revendored without re-declaring it, or the \
             digest was copied out of somewhere that has since moved. Fix {compat_file} with the \
             digest the build just computed.",
            self.abi_header_sha256
        );
    }

    /// Refuses to build against an archive the declaration does not cover.
    ///
    /// Both checks name `compat_file`, because that is the file somebody has
    /// to edit to say the new archive is fine, and neither of them is a fact
    /// about the archive being broken.
    pub fn check_archive(&self, identity: &ArchiveIdentity, manifest: &Path, compat_file: &str) {
        assert!(
            glob_matches(&self.native_artifact_versions, &identity.artifact_version),
            "{compat_file} says `native_artifact_versions = \"{}\"`, and the archive at {} says \
             its `artifact_version` is \"{}\". I will not link an archive this crate has not \
             declared itself compatible with. If the new archive is the one you want, widen or \
             move the glob in {compat_file} and re-run the tests that call the library.",
            self.native_artifact_versions,
            manifest.display(),
            identity.artifact_version
        );
        assert!(
            self.acadsharp_versions
                .contains(&identity.acadsharp_version),
            "{compat_file} lists `acadsharp_versions = {:?}`, and the archive at {} was built \
             from ACadSharp \"{}\". The upstream version is what decides whether the decoded \
             output still means what the frozen outputs say it means, so an unlisted one stops \
             here rather than at a fixture diff nobody reads. Add it to {compat_file} once the \
             bump has been through the runbook in docs/UPGRADING.md.",
            self.acadsharp_versions,
            manifest.display(),
            identity.acadsharp_version
        );
    }

    /// The declaration as the markdown table `README.md` carries.
    pub fn readme_table(&self) -> String {
        let values = [
            format!("`{}`", self.abi_version),
            format!("`{}`", self.wire_version),
            format!("`{}`", self.abi_header_sha256),
            format!("`{}`", self.native_artifact_versions),
            self.acadsharp_versions
                .iter()
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(", "),
        ];
        let mut out = String::from("| What `COMPAT.toml` declares | Value | What checks it |\n");
        out.push_str("| --- | --- | --- |\n");
        for ((key, value), checked) in KEYS.iter().zip(values.iter()).zip(CHECKED_AGAINST.iter()) {
            out.push_str(&format!("| `{key}` | {value} | {checked} |\n"));
        }
        out
    }
}

/// The two `metadata/LINKINFO.json` fields the declaration has an opinion
/// about.
///
/// It is a struct rather than a pair of arguments so that whoever produced the
/// two strings is nobody's business here. That is the whole point of it: this
/// half of the build script checks an archive somebody else has already found,
/// validated and parsed, and it reads no environment variable, resolves no
/// path and prints nothing of its own.
///
/// Issue #3 lands the real manifest parser under `serde`. When it does, it
/// builds one of these out of its own `LinkInfo` in one line:
///
/// ```ignore
/// ArchiveIdentity::new(&info.artifact_version, &info.acadsharp_version)
/// ```
///
/// and [`ArchiveIdentity::from_manifest_text`] below goes away, with
/// [`Compat::check_archive`] and every test around it untouched. A
/// `From<&LinkInfo>` would be nicer still and cannot be written here, because
/// that type does not exist on this branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveIdentity {
    /// `<upstream>-viprs.<revision>`, e.g. `3.7.1-viprs.1`.
    pub artifact_version: String,
    /// The upstream ACadSharp version alone, e.g. `3.7.1`.
    pub acadsharp_version: String,
}

impl ArchiveIdentity {
    /// The two fields, from whoever already has them.
    ///
    /// This is the seam. It takes strings rather than a manifest so that the
    /// parser producing them is somebody else's decision, which is what lets
    /// the hand reader below be deleted without a single test moving.
    pub fn new(
        artifact_version: impl Into<String>,
        acadsharp_version: impl Into<String>,
    ) -> ArchiveIdentity {
        ArchiveIdentity {
            artifact_version: artifact_version.into(),
            acadsharp_version: acadsharp_version.into(),
        }
    }

    /// Reads the two fields out of a manifest, by hand.
    ///
    /// **This is dead on compose.** Issue #3 ships `metadata/LINKINFO.json`
    /// parsed by `serde_json`, and the moment that lands this function and
    /// [`string_field`] below are deleted and [`ArchiveIdentity::new`] takes
    /// over. It is here only so this lane stands up on its own branch, and it
    /// reads exactly two flat string fields and understands nothing else,
    /// which is deliberately too little to be worth keeping.
    ///
    /// It is also not quite right, in the direction of refusing things a real
    /// parser accepts. It counts a field by looking for `"name"` followed by a
    /// colon, so a manifest that puts the same key inside a nested object,
    /// which `serde_json` reads fine because the outer key is one this crate
    /// skips, is refused here for saying it twice. Getting that right means
    /// tracking nesting, which means writing the parser issue #3 has already
    /// written.
    pub fn from_manifest_text(json: &str, manifest: &Path) -> ArchiveIdentity {
        ArchiveIdentity {
            artifact_version: string_field(json, "artifact_version", manifest),
            acadsharp_version: string_field(json, "acadsharp_version", manifest),
        }
    }
}

/// Does `value` match `pattern`, where `*` in the pattern stands for any run
/// of characters including an empty one?
///
/// Anchored at both ends: `3.7.1` matches `3.7.1` and nothing else. Everything
/// that is not a `*` is a literal, so the `.` in `3.7.1-viprs.*` is a real dot
/// and `3.7.1-viprs` (a version with no revision at all) does not match. That
/// is the decision the issue left open, and `COMPAT.toml` says so beside the
/// glob: the revision moves when the shim changes without upstream moving, so
/// an artifact version without one is an archive whose provenance I cannot
/// describe.
///
/// This is the classic two-pointer wildcard match with one backtrack point,
/// which is exact for any number of `*`. Bytes rather than chars is safe
/// because `*` is ASCII and cannot appear inside a UTF-8 multi-byte sequence.
pub fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut resume = 0usize;

    while v < value.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = v;
        } else if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if let Some(s) = star {
            // The last `*` swallows one more byte and we try again from there.
            p = s + 1;
            resume += 1;
            v = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// The text between the two generated-table markers in `README.md`.
pub fn readme_table_region<'a>(readme: &'a str, what: &str) -> &'a str {
    let (begin, end) = marker_offsets(readme, what);
    &readme[begin..end]
}

/// `readme` with the generated table replaced by `table`.
pub fn replace_readme_table(readme: &str, table: &str, what: &str) -> String {
    let (begin, end) = marker_offsets(readme, what);
    format!(
        "{}\n{}{}",
        readme[..begin].trim_end(),
        table,
        &readme[end..]
    )
}

/// Where the generated region starts and ends, refusing every shape that is
/// not exactly one well-formed pair of markers.
fn marker_offsets(readme: &str, what: &str) -> (usize, usize) {
    let count = |marker: &str| readme.matches(marker).count();
    assert!(
        count(README_TABLE_BEGIN) == 1 && count(README_TABLE_END) == 1,
        "{what} has to carry exactly one `{README_TABLE_BEGIN}` and one `{README_TABLE_END}`, and \
         it has {} and {}. The table between them is generated from {COMPAT_FILE}.",
        count(README_TABLE_BEGIN),
        count(README_TABLE_END)
    );
    let begin = readme
        .find(README_TABLE_BEGIN)
        .expect("counted one just above")
        + README_TABLE_BEGIN.len();
    let end = readme
        .find(README_TABLE_END)
        .expect("counted one just above");
    assert!(
        begin <= end,
        "{what} closes the generated table before it opens it."
    );
    (begin, end)
}

/// Where in the declaration something went wrong, for a refusal message.
#[derive(Clone, Copy)]
struct Where<'a> {
    what: &'a str,
    line: usize,
}

/// Records a key's value, refusing a second one.
fn set<T>(slot: &mut Option<T>, key: &str, at: Where<'_>, value: T) {
    assert!(
        slot.is_none(),
        "{} sets `{key}` twice, and line {} is the second. Two values for one key is a file to \
         fix rather than a value to pick: I would rather not guess which one the author meant.",
        at.what,
        at.line
    );
    *slot = Some(value);
}

/// A bare decimal integer that fits a `u32`.
fn integer(value: &str, key: &str, at: Where<'_>) -> u32 {
    assert!(
        !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "{} line {}: `{key}` is a plain decimal integer and `{value}` is not one.",
        at.what,
        at.line
    );
    value.parse::<u32>().unwrap_or_else(|e| {
        panic!(
            "{} line {}: `{key} = {value}` does not fit a u32, and the header's own field for it \
             is 32 bits wide: {e}",
            at.what, at.line
        )
    })
}

/// A quoted string with no escapes in it.
fn string(value: &str, key: &str, at: Where<'_>) -> String {
    let unquoted = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| {
            panic!(
                "{} line {}: `{key}` has to be a double-quoted string and `{value}` is not one.",
                at.what, at.line
            )
        });
    assert!(
        !unquoted.contains('"') && !unquoted.contains('\\'),
        "{} line {}: `{key}` contains a quote or a backslash, and this reader does not decode \
         escapes.",
        at.what,
        at.line
    );
    unquoted.to_string()
}

/// 64 lowercase hex characters, with no `0x` and no uppercase.
///
/// The format is checked here rather than at the comparison, because a digest
/// that differs only in case compares unequal and reads as a header that
/// moved, which sends somebody looking at the wrong file.
fn digest(value: &str, key: &str, at: Where<'_>) -> String {
    let text = string(value, key, at);
    assert!(
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{} line {}: `{key}` is a sha256 written as 64 lowercase hex characters with no `0x`, \
         and `{text}` is not that.",
        at.what,
        at.line
    );
    text
}

/// A single-line `["a", "b"]` array of quoted strings.
fn string_array(value: &str, key: &str, at: Where<'_>) -> Vec<String> {
    let body = value
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or_else(|| {
            panic!(
                "{} line {}: `{key}` has to be a single-line array of quoted strings, and \
                 `{value}` is not one.",
                at.what, at.line
            )
        });
    let mut out = Vec::new();
    for item in body.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        out.push(string(item, key, at));
    }
    out
}

/// Everything before an unquoted `#`.
///
/// The declaration carries a comment on every key saying what checks it, so
/// trailing comments have to work. Quoting is tracked so a `#` inside a string
/// stays in the string; escapes are not, because [`string`] refuses a
/// backslash outright.
fn strip_comment<'a>(line: &'a str, number: usize, what: &str) -> &'a str {
    let mut quoted = false;
    for (index, byte) in line.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b'#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    assert!(
        !quoted,
        "{what} line {number} opens a string it never closes."
    );
    line
}

/// One `"name": "value"` field out of a JSON document.
///
/// **Dead on compose**, along with [`ArchiveIdentity::from_manifest_text`]
/// above: issue #3's `serde_json` parser replaces both. Until then: the
/// manifest arrives inside a downloaded tarball, so a field that appears twice
/// is refused rather than resolved, and a control character is refused because
/// these strings end up in build output that a human then has to read.
///
/// # Counting at key position, and why that is still not enough
///
/// The count used to be over `"name"` anywhere in the text, and that was a
/// false refusal. `{"fields_this_consumer_reads": ["artifact_version", ...]}`
/// is valid JSON, `serde_json` reads it without a murmur (the outer key is one
/// this crate skips), and the count saw two and stopped the build. A build that
/// refuses something correct is worse than one that misses something wrong,
/// because nobody can act on it.
///
/// So an occurrence counts only when a colon follows it, which is the shape a
/// key has and a value does not. That still is not a JSON parser: the same key
/// inside a nested object is at key position too, and this refuses it. Getting
/// that one right means tracking nesting, which means writing the parser issue
/// #3 has already written, so I would rather leave the residue in something
/// about to be deleted than grow a second parser to cover it.
fn string_field(json: &str, field: &str, manifest: &Path) -> String {
    let key = format!("\"{field}\"");
    let at = key_positions(json, &key);
    assert!(
        at.len() == 1,
        "{} names `{field}` as a key {} times, and I read it exactly once or not at all.",
        manifest.display(),
        at.len()
    );

    let rest = &json[at[0] + key.len()..];
    let open = rest.find('"').unwrap_or_else(|| {
        panic!(
            "`{field}` in {} is not followed by a quoted string",
            manifest.display()
        )
    });
    assert!(
        rest[..open].trim() == ":",
        "`{field}` in {} is not a plain `\"{field}\": \"...\"` pair",
        manifest.display()
    );
    let tail = &rest[open + 1..];
    let close = tail.find('"').unwrap_or_else(|| {
        panic!(
            "`{field}` in {} opens a string it never closes",
            manifest.display()
        )
    });
    let value = &tail[..close];
    assert!(
        !value.contains('\\'),
        "`{field}` in {} contains an escape sequence, and this reader does not decode those",
        manifest.display()
    );
    assert!(
        !value.chars().any(char::is_control),
        "`{field}` in {} contains a control character, and this value ends up in build output a \
         human has to read",
        manifest.display()
    );
    assert!(
        !value.is_empty(),
        "`{field}` in {} is empty, which is a malformed manifest rather than a fact about the \
         archive",
        manifest.display()
    );
    value.to_string()
}

/// Every offset in `json` where `key` is followed by a colon, which is where a
/// JSON key sits and where a value does not.
///
/// Deleted with [`string_field`] at compose.
fn key_positions(json: &str, key: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = json[from..].find(key) {
        let at = from + offset;
        from = at + key.len();
        if json[from..].trim_start().starts_with(':') {
            found.push(at);
        }
    }
    found
}
