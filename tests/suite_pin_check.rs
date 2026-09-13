//! Drives `tools/suite_pin_check.sh` over every combination of ref kind and
//! compare status, plus the shape of `SUITE_REV` and of the `suite` job that
//! calls the script.
//!
//! This is the part of H2.5 that can be proven from inside this repo without a
//! runner. The `main` arm of the rule cannot be reached by pushing a branch
//! (you would have to merge to get there), so the branch logic lives in a
//! script and the script is driven here. The job's cross-repo half is proven on
//! a real Actions run instead, now that `acadsharp-rs-tests` carries the
//! `acadsharp-rs = { path = "../acadsharp-rs" }` dependency back to this crate.
use std::path::PathBuf;
use std::process::Command;

/// Message payload only. The script never touches the network, so this is just
/// a realistic 40-hex string to look for in the output. It happens to be the
/// current `SUITE_REV`, which keeps a reader from wondering whether some other
/// commit matters.
const SHA: &str = "e31863f4778d85e891343b6457093ca03663dc23";

/// `compare/main...<sha>` statuses that mean the pin is on the suite's `main`.
const MERGED: &[&str] = &["identical", "behind"];

/// The two that mean it is not on the suite's `main`.
const UNMERGED: &[&str] = &["ahead", "diverged"];

/// Ref names that are not `main`. `42/merge` is what GitHub actually puts in
/// `GITHUB_REF_NAME` on a `pull_request` event, so it is in the matrix rather
/// than three hand-picked branch names that all look alike.
const PR_REFS: &[&str] = &["h25-suite", "42/merge", "feature/nested/name"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path() -> PathBuf {
    repo_root().join("tools").join("suite_pin_check.sh")
}

struct Run {
    code: Option<i32>,
    output: String,
}

impl Run {
    fn code(&self) -> i32 {
        self.code
            .expect("suite_pin_check.sh died on a signal instead of exiting")
    }

    fn says(&self, needle: &str) -> bool {
        self.output.contains(needle)
    }
}

/// Runs the script with `GITHUB_REF_NAME` set to `ref_name`, or removed from
/// the environment entirely when it is `None`. stdout and stderr are joined,
/// because which stream an annotation lands on is not the contract.
fn run(ref_name: Option<&str>, args: &[&str]) -> Run {
    let script = script_path();
    assert!(
        script.is_file(),
        "tools/suite_pin_check.sh does not exist at {}",
        script.display()
    );
    let mut cmd = Command::new(&script);
    cmd.args(args);
    cmd.env_remove("GITHUB_REF_NAME");
    if let Some(name) = ref_name {
        cmd.env("GITHUB_REF_NAME", name);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("could not run {}: {e}", script.display()));
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Run {
        code: out.status.code(),
        output,
    }
}

fn every_ref() -> Vec<&'static str> {
    let mut refs = vec!["main"];
    refs.extend_from_slice(PR_REFS);
    refs
}

#[test]
fn the_script_is_executable() {
    let script = script_path();
    assert!(
        script.is_file(),
        "tools/suite_pin_check.sh does not exist at {}",
        script.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = script
            .metadata()
            .expect("stat the script")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "tools/suite_pin_check.sh is not executable (mode {mode:o}); the job calls it directly"
        );
    }
}

#[test]
fn a_pin_on_suite_main_passes_on_every_ref() {
    for ref_name in every_ref() {
        for status in MERGED {
            let r = run(Some(ref_name), &[status, SHA]);
            assert_eq!(
                r.code(),
                0,
                "ref {ref_name} + status {status} should pass, got:\n{}",
                r.output
            );
            assert!(
                !r.says("::error"),
                "ref {ref_name} + status {status} printed an error:\n{}",
                r.output
            );
            // A merged pin must not look like an unmerged one. The notice is
            // the signal that a follow-up PR is owed, so printing it here would
            // train everyone to ignore it.
            assert!(
                !r.says("::notice"),
                "ref {ref_name} + status {status} printed a notice:\n{}",
                r.output
            );
        }
    }
}

#[test]
fn an_unmerged_pin_fails_on_main() {
    for status in UNMERGED {
        let r = run(Some("main"), &[status, SHA]);
        assert_eq!(
            r.code(),
            1,
            "status {status} on main should exit 1, got:\n{}",
            r.output
        );
        assert!(
            r.says("::error"),
            "no ::error for {status} on main:\n{}",
            r.output
        );
        assert!(
            r.says("SUITE_REV"),
            "the refusal does not name SUITE_REV:\n{}",
            r.output
        );
        assert!(
            r.says(SHA),
            "the refusal does not name the sha:\n{}",
            r.output
        );
        assert!(
            r.says(status),
            "the refusal does not name the status:\n{}",
            r.output
        );
    }
}

#[test]
fn an_unmerged_pin_passes_on_a_pr_ref_with_a_notice() {
    for ref_name in PR_REFS {
        for status in UNMERGED {
            let r = run(Some(ref_name), &[status, SHA]);
            assert_eq!(
                r.code(),
                0,
                "ref {ref_name} + status {status} should pass, got:\n{}",
                r.output
            );
            assert!(
                r.says("::notice"),
                "ref {ref_name} + status {status} printed no notice:\n{}",
                r.output
            );
            assert!(
                !r.says("::error"),
                "ref {ref_name} + status {status} printed an error:\n{}",
                r.output
            );
            assert!(
                r.says(SHA),
                "the notice does not name the sha:\n{}",
                r.output
            );
            assert!(
                r.says(status),
                "the notice does not name the status:\n{}",
                r.output
            );
        }
    }
}

#[test]
fn an_unknown_compare_status_is_refused_on_every_ref() {
    // Not a status GitHub returns. Treating it as merged would turn a typo, a
    // renamed field or an API change into a silent pass, which is the failure
    // this whole job exists to stop.
    for ref_name in every_ref() {
        for status in ["unknown", "MERGED", "ok", "null"] {
            let r = run(Some(ref_name), &[status, SHA]);
            assert_eq!(
                r.code(),
                1,
                "ref {ref_name} + status {status} should exit 1, got:\n{}",
                r.output
            );
            assert!(
                r.says("::error"),
                "ref {ref_name} + status {status} printed no error:\n{}",
                r.output
            );
        }
    }
}

#[test]
fn an_empty_compare_status_is_a_usage_error() {
    // `gh api ... -q .status` prints nothing when the field is missing, so this
    // is what a broken query looks like from here. Exit 2 rather than 1, so a
    // wiring mistake reads differently from a policy refusal.
    for ref_name in every_ref() {
        let r = run(Some(ref_name), &["", SHA]);
        assert_eq!(r.code(), 2, "empty status on {ref_name}:\n{}", r.output);
        assert!(
            r.says("::error"),
            "empty status printed no error:\n{}",
            r.output
        );
    }
}

#[test]
fn a_missing_ref_name_is_refused() {
    // The org has been burnt by an unset variable reading as the empty string
    // and then as "the default branch". Empty is not `main` and it is not a PR
    // branch either, so the script refuses instead of picking one.
    for ref_name in [None, Some("")] {
        for status in MERGED.iter().chain(UNMERGED) {
            let r = run(ref_name, &[status, SHA]);
            assert_eq!(
                r.code(),
                2,
                "ref {ref_name:?} + status {status} should exit 2, got:\n{}",
                r.output
            );
            assert!(
                r.says("GITHUB_REF_NAME"),
                "the refusal does not name GITHUB_REF_NAME:\n{}",
                r.output
            );
        }
    }
}

#[test]
fn the_wrong_number_of_arguments_is_a_usage_error() {
    for args in [&[][..], &["identical", SHA, "extra"][..]] {
        let r = run(Some("main"), args);
        assert_eq!(
            r.code(),
            2,
            "args {args:?} should exit 2, got:\n{}",
            r.output
        );
        assert!(
            r.says("::error"),
            "args {args:?} printed no error:\n{}",
            r.output
        );
    }
}

#[test]
fn the_sha_argument_is_optional() {
    let r = run(Some("main"), &["identical"]);
    assert_eq!(
        r.code(),
        0,
        "a merged status with no sha should pass:\n{}",
        r.output
    );
}

/// The first line of `SUITE_REV` that is neither blank nor a comment. Same rule
/// the workflow's awk uses, written twice on purpose so a change to one of them
/// shows up here.
fn suite_rev_payload(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[test]
fn suite_rev_is_comments_then_exactly_one_lowercase_sha() {
    let path = repo_root().join("SUITE_REV");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));

    assert!(
        text.lines().any(|l| l.trim_start().starts_with('#')),
        "SUITE_REV carries no comment explaining the rule"
    );

    let payload = suite_rev_payload(&text);
    assert_eq!(
        payload.len(),
        1,
        "SUITE_REV must hold exactly one non-comment line, found {payload:?}"
    );

    let sha = payload[0];
    assert_eq!(
        sha.len(),
        40,
        "SUITE_REV holds {sha:?}, which is not 40 characters"
    );
    assert!(
        sha.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "SUITE_REV holds {sha:?}, which is not lowercase hex; a branch name is never allowed here"
    );
}

/// The `suite:` job block, from its key to the next job key at the same indent.
fn suite_job_block(workflow: &str) -> String {
    let mut block = Vec::new();
    let mut inside = false;
    for line in workflow.lines() {
        if line.starts_with("  suite:") {
            inside = true;
            block.push(line);
            continue;
        }
        if inside {
            // A sibling job key: two spaces of indent and then a name.
            let is_sibling = line.starts_with("  ")
                && !line.starts_with("   ")
                && line.trim_end().ends_with(':')
                && !line.trim_start().starts_with('#');
            if is_sibling {
                break;
            }
            block.push(line);
        }
    }
    assert!(
        inside,
        "there is no `suite:` job in .github/workflows/ci.yml"
    );
    block.join("\n")
}

#[test]
fn the_suite_job_is_wired_the_way_the_rule_needs() {
    let path = repo_root().join(".github/workflows/ci.yml");
    let workflow = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let job = suite_job_block(&workflow);

    // The context name branch protection will be asked to require. Renaming it
    // later leaves the old name required and blocks every PR, which is the trap
    // the `msrv` and `docs` job comments already record.
    assert!(
        job.contains("name: Suite (acadsharp-rs-tests)"),
        "the suite job is not named `Suite (acadsharp-rs-tests)`:\n{job}"
    );
    assert!(
        job.contains("tools/suite_pin_check.sh"),
        "the suite job does not call the script the branch rule lives in:\n{job}"
    );
    assert!(
        job.contains("VIPRS_REQUIRE_COUNTERPART: 1"),
        "the suite job does not set VIPRS_REQUIRE_COUNTERPART, so the suite could skip:\n{job}"
    );
    assert!(
        job.contains("VIPRS_COUNTERPART_EXPECTED_SHA: ${{ github.sha }}"),
        "the suite job does not tell the suite which sibling sha to expect:\n{job}"
    );
    // Fetch by sha, no branch and no fallback.
    assert!(
        job.contains("fetch --depth 1 origin"),
        "the suite job does not fetch the suite by sha:\n{job}"
    );
    assert!(
        job.contains("rev-parse HEAD"),
        "the suite job does not verify the checked-out sha against the pin:\n{job}"
    );

    // The "does the suite point back at this crate" check asks cargo, not a
    // regex. A grep over the manifest proves the declaration and not the
    // resolution: a `[patch]`, a workspace member, `optional = true` or the
    // entry under `[dev-dependencies]` all satisfy a text match while changing
    // what gets built. So the job has to be reading the resolve graph.
    assert!(
        job.contains("cargo metadata"),
        "the suite job does not ask cargo where acadsharp-rs resolved from:\n{job}"
    );
    assert!(
        job.contains("dep_kinds"),
        "the suite job does not check that acadsharp-rs is a normal dependency \
         rather than a dev or optional one:\n{job}"
    );

    // Everything native is `cfg(acadsharp_linked)`, so a suite run with no
    // archive compiles those tests out and goes green having run none of them.
    assert!(
        job.contains("./acadsharp-rs/.github/actions/fetch-native-archive"),
        "the suite job runs the suite with no native archive, so a native test \
         in the suite would compile out and this job would stay green:\n{job}"
    );

    // A skipped check is the same colour as a passing one, and a job that
    // swallows its own failure is worse. Neither is allowed in here. Matched as
    // YAML keys rather than as substrings, so the job is still free to explain
    // in a comment or an error message why it has neither of them.
    for line in job.lines() {
        let key = line.trim_start().trim_start_matches("- ");
        assert!(
            !key.starts_with("if:"),
            "the suite job carries a conditional, which would let it skip:\n{line}"
        );
        assert!(
            !key.starts_with("continue-on-error:"),
            "the suite job swallows a failure:\n{line}"
        );
    }
}

// ---------------------------------------------------------------------------
// The composite action that lays the native archive down
// ---------------------------------------------------------------------------

fn action_path() -> PathBuf {
    repo_root().join(".github/actions/fetch-native-archive/action.yml")
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// The archive's sha256, as the action pins it. Written out here on purpose:
/// this test is about the pin living in exactly one place, so it needs its own
/// copy of what to look for.
const ARCHIVE_SHA256: &str = "4cbdbbdd03c4431780ddca80a1d660e6940ff561734d666e162d6fb50438136e";

#[test]
fn the_native_archive_action_verifies_what_it_downloads() {
    let action = read(&action_path());

    assert!(
        action.contains("using: composite"),
        "the archive fetch has to be a composite action for a job to call it:\n{action}"
    );
    assert!(
        action.contains(ARCHIVE_SHA256),
        "the action does not pin the archive's sha256, so it would trust whatever it downloaded"
    );
    assert!(
        action.contains("sha256sum -c -"),
        "the action does not verify the archive it downloaded"
    );
    assert!(
        action.contains("ACADSHARP_NATIVE_DIR="),
        "the action does not export ACADSHARP_NATIVE_DIR, so nothing downstream links"
    );

    // The verify step must not be behind the cache hit. A cache entry that came
    // back wrong is exactly the case the digest exists for, and skipping the
    // check for a local file is how a poisoned cache becomes a green run.
    let verify = action
        .split("- name: Verify and unpack the native archive")
        .nth(1)
        .expect("the action has no verify-and-unpack step");
    for line in verify.lines() {
        let key = line.trim_start().trim_start_matches("- ");
        assert!(
            !key.starts_with("if:"),
            "the verify step is conditional, so a bad cache entry would be trusted:\n{line}"
        );
    }
}

#[test]
fn the_native_archive_pin_lives_in_exactly_one_place() {
    // The release tag, the archive name and its digest have to move together,
    // so they get one home. `Test` fetches the same archive (that half is
    // acadsharp-rs#1's PR, still in flight at the time of writing), and if it
    // keeps its own inline copy of these three lines they will drift the first
    // time the pin moves. This fails when a second copy appears in `ci.yml`,
    // and the fix is to call the action from that job too.
    let workflow = read(&repo_root().join(".github/workflows/ci.yml"));
    assert!(
        !workflow.contains(ARCHIVE_SHA256),
        "ci.yml carries its own copy of the native archive's sha256. That pin lives in \
         .github/actions/fetch-native-archive/action.yml; replace the inline download steps \
         with `uses: ./.github/actions/fetch-native-archive` so the two cannot drift."
    );
    assert!(
        !workflow.contains("releases/download"),
        "ci.yml downloads a release artifact directly. That belongs in \
         .github/actions/fetch-native-archive/action.yml, which every job that needs the \
         native lane calls."
    );
}
