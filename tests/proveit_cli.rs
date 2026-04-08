use std::fs;
use std::path::{Path, PathBuf};
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

fn write_manifest(repo: &Path, feature_id: &str, watch_path: &str, proofs: &str) -> PathBuf {
    let path = repo
        .join(".proveit")
        .join("manifests")
        .join(format!("{feature_id}.toml"));
    let body = format!(
        r#"[feature]
id = "{feature_id}"
owner = "tests"
watch_paths = ["{watch_path}"]
required_score = 2

{proofs}
"#
    );
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn verify_persists_artifacts_and_report_can_close() {
    let repo = temp_repo();
    write_manifest(
        repo.path(),
        "demo-feature",
        "src/watched.txt",
        r#"[[proofs]]
id = "positive"
category = "positive"
description = "prints a success marker"
command = "printf 'proof-ok\n'"
timeout_secs = 30

[[proofs]]
id = "artifact"
category = "artifact"
description = "emits a JSON artifact"
command = "printf '{\"answer\":42}\n'"
timeout_secs = 30
artifact_extract = "json"
"#,
    );

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
    write_manifest(
        repo.path(),
        "stale-feature",
        "src/watched.txt",
        r#"[[proofs]]
id = "positive"
category = "positive"
description = "prints success"
command = "printf 'ok\n'"
timeout_secs = 30

[[proofs]]
id = "artifact"
category = "artifact"
description = "prints json"
command = "printf '{\"fresh\":true}\n'"
timeout_secs = 30
artifact_extract = "json"
"#,
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

#[test]
fn verify_impacted_only_runs_matching_manifests() {
    let repo = temp_repo();
    write_manifest(
        repo.path(),
        "feature-a",
        "src/watched.txt",
        r#"[[proofs]]
id = "positive"
category = "positive"
description = "a"
command = "printf 'a\n'"
timeout_secs = 30

[[proofs]]
id = "artifact"
category = "artifact"
description = "a json"
command = "printf '{\"feature\":\"a\"}\n'"
timeout_secs = 30
artifact_extract = "json"
"#,
    );
    write_manifest(
        repo.path(),
        "feature-b",
        "src/other.txt",
        r#"[[proofs]]
id = "positive"
category = "positive"
description = "b"
command = "printf 'b\n'"
timeout_secs = 30

[[proofs]]
id = "artifact"
category = "artifact"
description = "b json"
command = "printf '{\"feature\":\"b\"}\n'"
timeout_secs = 30
artifact_extract = "json"
"#,
    );

    fs::write(repo.path().join("src").join("watched.txt"), "changed\n").unwrap();

    let output = run_dynamic(repo.path(), &[proveit_bin(), "--json", "verify-impacted"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let features = json["features"].as_array().unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["feature_id"].as_str(), Some("feature-a"));
    assert!(
        repo.path()
            .join(".proveit")
            .join("artifacts")
            .join("feature-a")
            .exists()
    );
    assert!(
        !repo
            .path()
            .join(".proveit")
            .join("artifacts")
            .join("feature-b")
            .exists()
    );
}

#[test]
fn verify_waits_for_existing_lock_to_clear() {
    let repo = temp_repo();
    write_manifest(
        repo.path(),
        "locked-feature",
        "src/watched.txt",
        r#"[[proofs]]
id = "positive"
category = "positive"
description = "prints a success marker"
command = "printf 'proof-ok\n'"
timeout_secs = 30
"#,
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
