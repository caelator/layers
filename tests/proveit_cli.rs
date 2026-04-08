#![cfg(feature = "integration")]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

fn proveit_bin() -> &'static str {
    env!("CARGO_BIN_EXE_proveit")
}

fn temp_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp repo");
    run(dir.path(), &["git", "init"]);
    run(
        dir.path(),
        &["git", "config", "user.email", "proveit@example.com"],
    );
    run(dir.path(), &["git", "config", "user.name", "Prove It"]);
    fs::create_dir_all(dir.path().join(".proveit").join("manifests")).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src").join("watched.txt"), "alpha\n").unwrap();
    run(dir.path(), &["git", "add", "."]);
    run(dir.path(), &["git", "commit", "-m", "initial"]);
    dir
}

fn run(cwd: &Path, args: &[&str]) {
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .status()
        .expect("failed to run command");
    assert!(status.success(), "command {:?} failed", args);
}

fn run_dynamic(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()
        .expect("failed to run command")
}

/// Write manifest TOML bytes directly to disk, bypassing Rust string escaping.
fn write_manifest_bytes(repo: &Path, feature_id: &str, watch_path: &str, body: &[u8]) {
    let path = repo
        .join(".proveit")
        .join("manifests")
        .join(format!("{feature_id}.toml"));
    let header = format!(
        "[feature]\n\
         id = \"{fid}\"\n\
         owner = \"tests\"\n\
         watch_paths = [\"{wp}\"]\n\
         required_score = 2\n\
         \n",
        fid = feature_id,
        wp = watch_path,
    );
    let mut full = header.into_bytes();
    full.extend_from_slice(body);
    fs::write(&path, full).unwrap();
}

/// Write a strict 5/5 manifest (all 5 proof categories present and passing).
/// The artifact proof uses `printf '%s\\n' '{"value":1}'` - the `\\n` in the TOML
/// becomes `\n` (backslash-n) when parsed, which printf interprets as a newline.
fn write_full_strict_manifest(repo: &Path, feature_id: &str, watch_path: &str) {
    write_manifest_bytes(
        repo,
        feature_id,
        watch_path,
        b"\
[[proofs]]\n\
id = \"positive\"\n\
category = \"positive\"\n\
description = \"positive proof\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
\n\
[[proofs]]\n\
id = \"counterfactual\"\n\
category = \"counterfactual\"\n\
description = \"counterfactual proof\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
\n\
[[proofs]]\n\
id = \"artifact\"\n\
category = \"artifact\"\n\
description = \"artifact proof\"\n\
command = \"printf '%s\\n' '{\\\"value\\\":1}'\"\n\
timeout_secs = 30\n\
artifact_extract = \"json\"\n\
\n\
[[proofs]]\n\
id = \"failure\"\n\
category = \"failure\"\n\
description = \"failure mode proof\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
\n\
[[proofs]]\n\
id = \"repeatability\"\n\
category = \"repeatability\"\n\
description = \"repeatability proof\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
",
    );
}

/// Write a partial manifest (positive + artifact, score 2, not strict).
fn write_partial_manifest(repo: &Path, feature_id: &str, watch_path: &str) {
    write_manifest_bytes(
        repo,
        feature_id,
        watch_path,
        b"\
[[proofs]]\n\
id = \"positive\"\n\
category = \"positive\"\n\
description = \"positive proof\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
\n\
[[proofs]]\n\
id = \"artifact\"\n\
category = \"artifact\"\n\
description = \"artifact proof\"\n\
command = \"printf '%s\\n' '{\\\"answer\\\":42}'\"\n\
timeout_secs = 30\n\
artifact_extract = \"json\"\n\
",
    );
}

#[test]
fn verify_persists_artifacts_and_report_can_close() {
    let repo = temp_repo();
    write_partial_manifest(repo.path(), "demo-feature", "src/watched.txt");

    let verify = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "verify", "demo-feature"],
    );
    assert!(
        verify.status.success(),
        "verify failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );

    let report: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(report["features"][0]["score"].as_u64(), Some(2));
    assert_eq!(report["features"][0]["may_close"].as_bool(), Some(true));
    assert_eq!(
        report["features"][0]["missing_categories"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let artifact_dir = repo
        .path()
        .join(".proveit")
        .join("artifacts")
        .join("demo-feature")
        .join("artifact");
    assert!(artifact_dir.exists(), "artifact directory must exist");

    let saved_report = repo
        .path()
        .join(".proveit")
        .join("verdicts")
        .join("demo-feature.json");
    assert!(saved_report.exists(), "verdict snapshot must exist");
}

#[test]
fn report_marks_proofs_stale_after_watched_change() {
    let repo = temp_repo();
    write_manifest_bytes(
        repo.path(),
        "stale-feature",
        "src/watched.txt",
        b"\
[[proofs]]\n\
id = \"positive\"\n\
category = \"positive\"\n\
description = \"prints success\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
\n\
[[proofs]]\n\
id = \"artifact\"\n\
category = \"artifact\"\n\
description = \"prints json\"\n\
command = \"printf '%s\\n' '{\\\"fresh\\\":true}'\"\n\
timeout_secs = 30\n\
artifact_extract = \"json\"\n\
",
    );

    let verify = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "verify", "stale-feature"],
    );
    assert!(verify.status.success());

    fs::write(repo.path().join("src").join("watched.txt"), "beta\n").unwrap();

    let report = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "report", "stale-feature"],
    );
    assert!(report.status.success());
    let json: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(json["features"][0]["stale"].as_bool(), Some(true));
    assert_eq!(json["features"][0]["may_close"].as_bool(), Some(false));
    assert_eq!(
        json["features"][0]["changed_files"][0].as_str(),
        Some("src/watched.txt")
    );

    let enforce = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "enforce", "stale-feature"],
    );
    assert!(
        !enforce.status.success(),
        "enforce must fail once watched files have changed"
    );
}

/// `verify-impacted` must FAIL when impacted features are not 5/5 strict.
#[test]
fn verify_impacted_strict_gate_fails_when_features_not_5_5() {
    let repo = temp_repo();
    write_partial_manifest(repo.path(), "feature-a", "src/watched.txt");
    write_partial_manifest(repo.path(), "feature-b", "src/other.txt");

    fs::write(repo.path().join("src").join("watched.txt"), "changed\n").unwrap();

    let output = run_dynamic(repo.path(), &[proveit_bin(), "--json", "verify-impacted"]);
    assert!(
        !output.status.success(),
        "verify-impacted must fail when impacted features are not 5/5: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("strict gate failed"),
        "error must mention strict gate: {stderr}"
    );
}

/// `verify-impacted` must SUCCEED when all impacted features are 5/5 strict.
#[test]
fn verify_impacted_succeeds_when_all_features_are_5_5() {
    let repo = temp_repo();
    write_full_strict_manifest(repo.path(), "feature-a", "src/watched.txt");
    write_full_strict_manifest(repo.path(), "feature-b", "src/other.txt");

    fs::write(repo.path().join("src").join("watched.txt"), "changed\n").unwrap();
    run(repo.path(), &["git", "add", "."]);
    run(repo.path(), &["git", "commit", "-m", "modify watched file"]);

    let output = run_dynamic(repo.path(), &[proveit_bin(), "--json", "verify-impacted"]);
    assert!(
        output.status.success(),
        "verify-impacted must succeed when all impacted features are 5/5: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let features = json["features"].as_array().unwrap();
    assert_eq!(features.len(), 1, "only feature-a is impacted");
    assert_eq!(features[0]["feature_id"].as_str(), Some("feature-a"));
    assert_eq!(features[0]["strict"].as_bool(), Some(true));
    assert_eq!(features[0]["score"].as_u64(), Some(5));
}

/// `report` (all features) must FAIL when any feature is not 5/5.
#[test]
fn report_all_fails_when_features_not_5_5() {
    let repo = temp_repo();
    write_partial_manifest(repo.path(), "partial-feature", "src/watched.txt");

    let output = run_dynamic(repo.path(), &[proveit_bin(), "--json", "report"]);
    assert!(
        !output.status.success(),
        "report must fail when any feature is not 5/5: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("strict gate failed"),
        "error must mention strict gate: {stderr}"
    );
}

/// `report` (all features) must SUCCEED when all features are 5/5 strict.
#[test]
fn report_all_succeeds_when_all_features_are_5_5() {
    let repo = temp_repo();
    write_full_strict_manifest(repo.path(), "full-feature", "src/watched.txt");

    let verify = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "verify", "full-feature"],
    );
    assert!(
        verify.status.success(),
        "verify must succeed before report: {}",
        String::from_utf8_lossy(&verify.stderr)
    );

    let output = run_dynamic(repo.path(), &[proveit_bin(), "--json", "report"]);
    assert!(
        output.status.success(),
        "report must succeed when all features are 5/5: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["features"][0]["strict"].as_bool(), Some(true));
    assert_eq!(json["features"][0]["score"].as_u64(), Some(5));
}

/// `report <feature>` (single-feature) must succeed even when not 5/5.
#[test]
fn report_single_feature_allows_partial() {
    let repo = temp_repo();
    write_partial_manifest(repo.path(), "partial-feature", "src/watched.txt");

    let output = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "report", "partial-feature"],
    );
    assert!(
        output.status.success(),
        "report <feature> must succeed even for non-5/5 features: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `enforce <feature>` still enforces only the feature's required_score (not 5/5).
#[test]
fn enforce_allows_partial_when_required_score_is_met() {
    let repo = temp_repo();
    write_partial_manifest(repo.path(), "partial-feature", "src/watched.txt");

    let output = run_dynamic(
        repo.path(),
        &[proveit_bin(), "--json", "enforce", "partial-feature"],
    );
    assert!(
        output.status.success(),
        "enforce must pass when required_score is met even if not 5/5: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verify_waits_for_existing_lock_to_clear() {
    let repo = temp_repo();
    write_manifest_bytes(
        repo.path(),
        "locked-feature",
        "src/watched.txt",
        b"\
[[proofs]]\n\
id = \"positive\"\n\
category = \"positive\"\n\
description = \"prints a success marker\"\n\
command = \"echo ok\"\n\
timeout_secs = 30\n\
",
    );

    let lock_path = repo.path().join(".proveit").join("proveit.lock");
    fs::write(&lock_path, "{\"pid\":999,\"timestamp\":\"stale-ish\"}\n").unwrap();

    let lock_path_for_thread = lock_path.clone();
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        fs::remove_file(lock_path_for_thread).unwrap();
    });

    let output = Command::new(proveit_bin())
        .args(["--json", "verify", "locked-feature"])
        .env("PROVEIT_LOCK_TIMEOUT_SECS", "2")
        .current_dir(repo.path())
        .output()
        .expect("failed to run proveit verify");

    releaser.join().unwrap();

    assert!(
        output.status.success(),
        "verify should wait for the lock to clear: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["features"][0]["feature_id"].as_str(),
        Some("locked-feature")
    );
}
