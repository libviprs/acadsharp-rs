//! Issue #18: the suite pin check has to catch a `SUITE_REV` the suite's
//! `main` does not reach, not only one with the wrong shape.
//!
//! Two halves.
//!
//! The first drives the real "SUITE_REV obeys the merged-only rule" step,
//! lifted out of `.github/workflows/ci.yml`, with a stub `gh` that answers the
//! way `gh` 2.102 answers a compare across unrelated histories: the 404 body on
//! stdout, `gh: No common ancestor ... (HTTP 404)` on stderr, exit 1. That is
//! what a rewritten suite `main` produces. The step has to turn it into an
//! `::error file=SUITE_REV::` naming the pin, on every ref, because an unrelated
//! history can never become mergeable. The suite's own clone action already
//! does that for its pin; this job does not.
//!
//! The second drives `tools/suite_pin_check.sh reachable <git-dir> <sha>
//! [<pin-file>]` against two fixture repositories: one whose `main` was
//! rewritten from the root with identical trees, the exact shape of the real
//! rewrite, and one whose rewrite kept a shared ancestor, which is also what
//! the head of an open suite PR looks like. That mode is how `cargo test` and
//! the Docker path get a reachability check that does not need the GitHub API,
//! and it keeps the compare arm's three-way split, because a pin that is not on
//! `main` is not always wrong:
//!
//! - an ancestor of `main` passes on every ref, the way `identical` and
//!   `behind` do;
//! - a pin that shares history with `main` but is not on it is refused on
//!   `main` and allowed with a `::notice` on a PR ref, the way `ahead` and
//!   `diverged` are, because a crate PR pins the head of an open suite PR on
//!   purpose (README.md, the paired-change order);
//! - a pin with no common ancestor is refused on every ref, because an
//!   unrelated history can never become mergeable, and the refusal names the
//!   commit on `main` with the same tree, since that is where the pin should
//!   move.
//!
//! The optional pin-file argument only changes the file the annotation names,
//! so R1.3 can drive the same mode for `native/NATIVE_HEADER_REV`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The pin the real rewrite orphaned. Only a payload for the stub and the
/// messages; nothing here touches the network.
const ORPHAN: &str = "e31863f4778d85e891343b6457093ca03663dc23";

const STEP: &str = "- name: SUITE_REV obeys the merged-only rule";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn joined(out: &Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s
}

/// The `run: |` block of the merged-only step, dedented. Read from the real
/// workflow so a test of a copy cannot pass while the job does something else.
fn merged_only_step() -> String {
    let path = repo_root().join(".github/workflows/ci.yml");
    let workflow = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let lines: Vec<&str> = workflow.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start() == STEP)
        .unwrap_or_else(|| panic!("ci.yml has no step `{STEP}`"));
    let run_at = (start + 1..lines.len())
        .find(|&i| lines[i].trim_start().starts_with("run: |"))
        .unwrap_or_else(|| panic!("the step `{STEP}` has no `run: |` block"));
    let key_indent = lines[run_at].len() - lines[run_at].trim_start().len();
    let mut body = Vec::new();
    for l in &lines[run_at + 1..] {
        if !l.trim().is_empty() && l.len() - l.trim_start().len() <= key_indent {
            break;
        }
        body.push(*l);
    }
    let indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    body.iter()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A stub `gh` on PATH. `identical` answers like a merged pin; `unrelated`
/// answers like gh 2.102 does for `compare/main...<sha>` across a rewrite.
fn stub_gh(dir: &Path, mode: &str) {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = match mode {
        "identical" => "#!/bin/sh\necho identical\n".to_string(),
        "unrelated" => format!(
            "#!/bin/sh\n\
             printf '%s\\n' '{{\"message\":\"No common ancestor between main and {ORPHAN}.\",\"documentation_url\":\"https://docs.github.com/rest/commits/commits#compare-two-commits\",\"status\":\"404\"}}'\n\
             echo 'gh: No common ancestor between main and {ORPHAN}. (HTTP 404)' >&2\n\
             exit 1\n"
        ),
        other => panic!("no stub mode {other}"),
    };
    let path = bin.join("gh");
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs the real step the way the job does: bash, from a workspace that holds
/// this checkout as `acadsharp-rs/`, with the step's env.
fn run_step(name: &str, mode: &str, ref_name: &str) -> Output {
    let ws = scratch(name);
    std::os::unix::fs::symlink(repo_root(), ws.join("acadsharp-rs")).unwrap();
    stub_gh(&ws, mode);
    let step = ws.join("step.sh");
    std::fs::write(&step, merged_only_step()).unwrap();
    let path = format!(
        "{}:{}",
        ws.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("bash")
        .arg(&step)
        .current_dir(&ws)
        .env("PATH", path)
        .env("GH_TOKEN", "unused-by-the-stub")
        .env("SUITE_REPO", "libviprs/acadsharp-rs-tests")
        .env("SUITE_REV", ORPHAN)
        .env("GITHUB_REF_NAME", ref_name)
        .output()
        .expect("run the step under bash")
}

fn assert_named_refusal(out: &Output, ref_name: &str) {
    let all = joined(out);
    assert_ne!(
        out.status.code(),
        Some(0),
        "on {ref_name}, a SUITE_REV with no common ancestor passed the merged-only step:\n{all}"
    );
    assert!(
        all.contains("::error file=SUITE_REV::") && all.contains(ORPHAN),
        "on {ref_name}, the merged-only step went red without naming SUITE_REV or the pin. \
         A compare across unrelated histories (a rewritten suite main) has to reach a refusal \
         that says which pin is wrong, the way the suite's clone action does; instead the step \
         died inside `gh` and printed only:\n{all}"
    );
}

#[test]
fn issue_18_control_a_merged_suite_rev_passes_the_real_step() {
    let out = run_step("issue_18_control", "identical", "main");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the control failed, so the harness is wrong, not the step:\n{}",
        joined(&out)
    );
}

#[test]
fn issue_18_an_unrelated_history_is_refused_by_name_on_main() {
    let out = run_step("issue_18_unrelated_main", "unrelated", "main");
    assert_named_refusal(&out, "main");
}

#[test]
fn issue_18_an_unrelated_history_is_refused_by_name_on_a_pr_ref() {
    // On a PR ref an unmerged pin is allowed with a notice, because a paired
    // change needs the suite branch first. A pin on an unrelated history is not
    // an unmerged pin: it can never merge. So it is refused here too.
    let out = run_step("issue_18_unrelated_pr", "unrelated", "42/merge");
    assert_named_refusal(&out, "42/merge");
}

// ---------------------------------------------------------------------------
// The reachability mode, against fixtures with a rewritten main
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-09-13T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-09-13T00:00:00Z")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        joined(&out)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository whose `main` was rewritten from the root the way the suite's
/// was: every commit replaced by one with the same tree and a different sha,
/// so the old head shares no history at all with the new `main`. Returns the
/// git dir, the orphaned old head, and the commit on `main` with its tree.
fn rewritten_from_the_root(name: &str) -> (PathBuf, String, String) {
    let dir = scratch(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("f"), "one\n").unwrap();
    git(&dir, &["add", "f"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("f"), "two\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "second"]);
    let old_head = git(&dir, &["rev-parse", "HEAD"]);
    let first_tree = git(&dir, &["rev-parse", "HEAD~1^{tree}"]);
    let second_tree = git(&dir, &["rev-parse", "HEAD^{tree}"]);

    // The rewrite: same trees, new messages, so new shas and no shared history.
    let new_first = git(
        &dir,
        &["commit-tree", &first_tree, "-m", "first (rewritten)"],
    );
    let new_head = git(
        &dir,
        &[
            "commit-tree",
            &second_tree,
            "-p",
            &new_first,
            "-m",
            "second (rewritten)",
        ],
    );
    git(&dir, &["update-ref", "refs/heads/main", &new_head]);
    (dir.join(".git"), old_head, new_head)
}

/// A repository whose rewrite kept a shared ancestor, the shape libviprs-dep's
/// has: `main` is first -> second -> third (rewritten), and the old `third`
/// still hangs off `second`. The old head is related to `main` but not an
/// ancestor of it, which is also exactly what the head of an open suite PR
/// looks like from the crate's side. Returns the git dir, the old head, and
/// the new head with the same tree.
fn rewritten_from_the_middle(name: &str) -> (PathBuf, String, String) {
    let dir = scratch(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("f"), "one\n").unwrap();
    git(&dir, &["add", "f"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("f"), "two\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "second"]);
    std::fs::write(dir.join("f"), "three\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "third"]);
    let old_head = git(&dir, &["rev-parse", "HEAD"]);
    let second = git(&dir, &["rev-parse", "HEAD~1"]);
    let third_tree = git(&dir, &["rev-parse", "HEAD^{tree}"]);

    // The rewrite: same tree, same parent, new message, so a new sha that still
    // shares `first` and `second` with the old head.
    let new_head = git(
        &dir,
        &[
            "commit-tree",
            &third_tree,
            "-p",
            &second,
            "-m",
            "third (rewritten)",
        ],
    );
    git(&dir, &["update-ref", "refs/heads/main", &new_head]);
    (dir.join(".git"), old_head, new_head)
}

/// The two kinds of ref the mode has to answer on: a push to `main`, and a
/// pull_request event, where GITHUB_REF_NAME is `<number>/merge`.
const REFS: [&str; 2] = ["main", "42/merge"];

fn reachable(git_dir: &Path, sha: &str, ref_name: &str, pin_file: Option<&str>) -> Output {
    let mut cmd = Command::new(repo_root().join("tools/suite_pin_check.sh"));
    cmd.arg("reachable").arg(git_dir).arg(sha);
    if let Some(pin_file) = pin_file {
        cmd.arg(pin_file);
    }
    cmd.env("GITHUB_REF_NAME", ref_name)
        .output()
        .expect("run tools/suite_pin_check.sh")
}

fn assert_orphan_refused(out: &Output, ref_name: &str, orphan: &str, twin: &str) {
    let all = joined(out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "on {ref_name}, tools/suite_pin_check.sh has no way to refuse a SUITE_REV that main does \
         not reach (expected exit 1, a policy refusal). It said:\n{all}"
    );
    assert!(
        all.contains("::error file=SUITE_REV::") && all.contains(orphan),
        "on {ref_name}, the refusal does not name SUITE_REV and the pin:\n{all}"
    );
    assert!(
        all.contains(twin),
        "on {ref_name}, main carries {twin}, which has the same tree as the orphaned pin, and the \
         refusal does not name it. That is where the pin should move, so the message has to say \
         so, with the full sha, since that is what goes into the pin file:\n{all}"
    );
}

#[test]
fn issue_18_a_pin_on_the_rewritten_main_passes_the_reachability_check() {
    // An ancestor of main is allowed on every ref, the way `identical` and
    // `behind` are in the compare arm.
    let (git_dir, _, head) = rewritten_from_the_root("issue_18_fixture_head");
    for ref_name in REFS {
        let out = reachable(&git_dir, &head, ref_name, None);
        assert_eq!(
            out.status.code(),
            Some(0),
            "on {ref_name}, tools/suite_pin_check.sh cannot confirm that a pin on main is \
             reachable (expected exit 0). It said:\n{}",
            joined(&out)
        );
    }
}

#[test]
fn issue_18_a_pin_that_a_rewrite_orphaned_is_refused() {
    let (git_dir, orphan, twin) = rewritten_from_the_root("issue_18_fixture_orphan");
    let out = reachable(&git_dir, &orphan, "main", None);
    assert_orphan_refused(&out, "main", &orphan, &twin);
}

#[test]
fn issue_18_a_pin_that_a_rewrite_orphaned_is_refused_on_a_pr_ref() {
    // The PR-ref allowance is for a pin that is not merged yet. A pin with no
    // common ancestor can never merge, so the ref makes no difference here.
    let (git_dir, orphan, twin) = rewritten_from_the_root("issue_18_fixture_orphan_pr");
    let out = reachable(&git_dir, &orphan, "42/merge", None);
    assert_orphan_refused(&out, "42/merge", &orphan, &twin);
}

#[test]
fn issue_18_a_related_but_unmerged_pin_is_allowed_with_a_notice_on_a_pr_ref() {
    // The head of an open suite PR, or a rewrite that kept a shared ancestor:
    // merge-base finds one, is-ancestor says no. A crate PR pins the head of an
    // open suite PR on purpose, so refusing this here would make a paired
    // change impossible to test before it merges.
    let (git_dir, old_head, _) = rewritten_from_the_middle("issue_18_fixture_related_pr");
    let out = reachable(&git_dir, &old_head, "42/merge", None);
    let all = joined(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "on 42/merge, tools/suite_pin_check.sh refused a pin that shares history with main but is \
         not on it yet (expected exit 0 with a notice, the way the compare arm allows `ahead` and \
         `diverged` on a PR ref). It said:\n{all}"
    );
    assert!(
        all.contains("::notice file=SUITE_REV::") && all.contains(&old_head),
        "on 42/merge, an unmerged pin passed without the notice naming it, the one that says the \
         follow-up PR is owed once the suite PR merges:\n{all}"
    );
}

#[test]
fn issue_18_a_related_but_unmerged_pin_is_refused_on_main() {
    // The same pin on main: the follow-up PR that moves SUITE_REV onto the
    // suite's merge commit is owed, so this run is red, the way `ahead` and
    // `diverged` are on main.
    let (git_dir, old_head, _) = rewritten_from_the_middle("issue_18_fixture_related_main");
    let out = reachable(&git_dir, &old_head, "main", None);
    let all = joined(&out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "on main, tools/suite_pin_check.sh let through a pin that main does not reach \
         (expected exit 1, a policy refusal). It said:\n{all}"
    );
    assert!(
        all.contains("::error file=SUITE_REV::") && all.contains(&old_head),
        "on main, the refusal does not name SUITE_REV and the pin:\n{all}"
    );
}

#[test]
fn issue_18_the_pin_file_argument_names_the_file_the_annotation_points_at() {
    // R1.3 drives this same mode for native/NATIVE_HEADER_REV against a
    // libviprs-dep clone, so the annotation has to point at the file it was
    // given and not at SUITE_REV. The argument changes nothing else.
    let (git_dir, orphan, _) = rewritten_from_the_root("issue_18_fixture_pin_file");
    let out = reachable(&git_dir, &orphan, "main", Some("native/NATIVE_HEADER_REV"));
    let all = joined(&out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "with a pin file named, tools/suite_pin_check.sh did not refuse an orphaned pin \
         (expected exit 1). It said:\n{all}"
    );
    assert!(
        all.contains("::error file=native/NATIVE_HEADER_REV::") && all.contains(&orphan),
        "the refusal has to point at the pin file it was given, native/NATIVE_HEADER_REV, and \
         name the pin:\n{all}"
    );
    assert!(
        !all.contains("file=SUITE_REV"),
        "the refusal still points at SUITE_REV, which is not the file this pin lives in:\n{all}"
    );
}
