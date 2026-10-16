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
//! The second drives `tools/suite_pin_check.sh reachable <git-dir> <sha>`
//! against a fixture repository whose `main` was rewritten with identical
//! trees, the exact shape of the real rewrite. That mode is how `cargo test`
//! and the Docker path get a reachability check that does not need the GitHub
//! API.
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
        .map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() })
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
// The reachability mode, against a fixture with a rewritten main
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
    assert!(out.status.success(), "git {args:?} failed: {}", joined(&out));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository whose `main` was rewritten the way the suite's was: every
/// commit replaced by one with the same tree and a different sha. Returns the
/// git dir, the orphaned old head, and the new head with the same tree.
fn rewritten_fixture(name: &str) -> (PathBuf, String, String) {
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
    let new_first = git(&dir, &["commit-tree", &first_tree, "-m", "first (rewritten)"]);
    let new_head = git(
        &dir,
        &["commit-tree", &second_tree, "-p", &new_first, "-m", "second (rewritten)"],
    );
    git(&dir, &["update-ref", "refs/heads/main", &new_head]);
    (dir.join(".git"), old_head, new_head)
}

fn reachable(git_dir: &Path, sha: &str) -> Output {
    Command::new(repo_root().join("tools/suite_pin_check.sh"))
        .arg("reachable")
        .arg(git_dir)
        .arg(sha)
        .env("GITHUB_REF_NAME", "main")
        .output()
        .expect("run tools/suite_pin_check.sh")
}

#[test]
fn issue_18_a_pin_that_a_rewrite_orphaned_is_refused() {
    let (git_dir, orphan, _) = rewritten_fixture("issue_18_fixture_orphan");
    let out = reachable(&git_dir, &orphan);
    let all = joined(&out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "tools/suite_pin_check.sh has no way to refuse a SUITE_REV that main does not reach \
         (expected exit 1, a policy refusal). It said:\n{all}"
    );
    assert!(
        all.contains("::error file=SUITE_REV::") && all.contains(&orphan),
        "the refusal does not name SUITE_REV and the pin:\n{all}"
    );
}

#[test]
fn issue_18_a_pin_on_the_rewritten_main_passes_the_reachability_check() {
    let (git_dir, _, head) = rewritten_fixture("issue_18_fixture_head");
    let out = reachable(&git_dir, &head);
    assert_eq!(
        out.status.code(),
        Some(0),
        "tools/suite_pin_check.sh cannot confirm that a pin on main is reachable \
         (expected exit 0). It said:\n{}",
        joined(&out)
    );
}
