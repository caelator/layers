//! End-to-end tests for `layers query` UC semantic retrieval behavior and gate integration.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn layers_bin() -> &'static str {
    env!("CARGO_BIN_EXE_layers")
}

fn make_home() -> TempDir {
    tempfile::tempdir().expect("failed to create temp HOME")
}

fn write_uc_config(home: &Path) {
    let config_dir = home.join(".memoryport");
    std::fs::create_dir_all(&config_dir).expect("failed to create ~/.memoryport");
    std::fs::write(config_dir.join("uc.toml"), "[uc]\n").expect("failed to write uc config");
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, perms).expect("failed to chmod fake uc");
}

fn write_fake_uc(home: &Path, body: &str) -> PathBuf {
    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    let fake_uc = bin_dir.join("uc");
    std::fs::write(&fake_uc, body).expect("failed to write fake uc");
    #[cfg(unix)]
    make_executable(&fake_uc);
    fake_uc
}

fn write_cargo_audit_wrapper(home: &Path) -> PathBuf {
    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create wrapper bin dir");
    let wrapper = bin_dir.join("cargo");
    let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = \"audit\" ]; then\n  if [ \"$2\" = \"--version\" ]; then\n    echo 'cargo-audit 0.0.0-test'\n  else\n    echo 'audit ok'\n  fi\n  exit 0\nfi\nexec \"{real_cargo}\" \"$@\"\n"
    );
    std::fs::write(&wrapper, body).expect("failed to write cargo wrapper");
    #[cfg(unix)]
    make_executable(&wrapper);
    wrapper
}

/// Seeds the openclaw config into the temp home. Returns false if the source
/// config doesn't exist (e.g. in CI), signalling the caller to skip.
fn seed_openclaw_config(home: &Path) -> bool {
    let source = Path::new("/Users/bri/.openclaw/openclaw.json");
    if !source.exists() {
        eprintln!(
            "Skipping: openclaw config not found at {}",
            source.display()
        );
        return false;
    }
    let config_dir = home.join(".openclaw");
    std::fs::create_dir_all(&config_dir).expect("failed to create temp ~/.openclaw");
    let target = config_dir.join("openclaw.json");
    std::fs::copy(source, target).expect("failed to seed openclaw config");
    true
}

fn base_query_command(home: &Path) -> Command {
    let mut cmd = Command::new(layers_bin());
    cmd.current_dir(repo_root())
        .env("HOME", home)
        .env("LAYERS_WORKSPACE_ROOT", repo_root())
        .args(["query", "selective state space models", "--json"]);
    cmd
}

/// End-to-end test: low-confidence retrieval falls back cleanly and tags
/// the JSON response as keyword-based fallback when UC is unavailable.
#[test]
fn test_uc_fallback_tagging() {
    let home = make_home();

    let output = base_query_command(home.path())
        .output()
        .expect("failed to execute layers query");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "layers query should succeed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert_eq!(json["route"].as_str(), Some("neither"));
    assert_eq!(json["confidence"].as_str(), Some("low"));
    assert_eq!(json["low_confidence_fallback"].as_bool(), Some(true));
    assert_eq!(
        json["retrieval_meta"]["memory_source"].as_str(),
        Some("keyword-low-confidence-fallback")
    );

    let fallback_reason = json["retrieval_meta"]["fallback_reason"]
        .as_str()
        .expect("fallback_reason should be present");
    assert!(
        fallback_reason.contains("uc is unavailable"),
        "expected unavailable UC fallback reason, got: {fallback_reason}"
    );

    let evidence = json["evidence"].as_str().unwrap_or("");
    assert!(
        evidence.contains("### Memory"),
        "expected memory section in evidence, got: {evidence}"
    );
    assert!(
        evidence.contains("- ["),
        "expected source-tagged evidence lines, got: {evidence}"
    );
}

/// End-to-end test: a UC retrieval that succeeds with fewer than the CLI
/// threshold emits the `--uc-min-results` warning in `open_uncertainty`.
#[test]
fn test_uc_min_results_warning() {
    let home = make_home();
    write_uc_config(home.path());
    let fake_uc = write_fake_uc(home.path(), "#!/bin/sh\necho 'semantic fact one'\n");

    let bin_dir = fake_uc.parent().expect("fake uc should have a parent");
    let original_path = std::env::var("PATH").unwrap_or_default();

    let output = base_query_command(home.path())
        .env("PATH", format!("{}:{original_path}", bin_dir.display()))
        .arg("--uc-min-results")
        .arg("5")
        .output()
        .expect("failed to execute layers query");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "layers query should succeed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert_eq!(json["low_confidence_fallback"].as_bool(), Some(true));
    assert_eq!(
        json["retrieval_meta"]["memory_source"].as_str(),
        Some("uc-low-confidence-fallback")
    );
    assert_eq!(
        json["retrieval_meta"]["fallback_reason"].as_str(),
        None,
        "successful UC retrieval should not expose a fallback reason"
    );

    let open_uncertainty = json["open_uncertainty"]
        .as_array()
        .expect("open_uncertainty should be an array");
    let has_threshold_warning = open_uncertainty.iter().any(|item| {
        item.as_str().is_some_and(|line| {
            line.contains("UC semantic retrieval returned 1 result")
                && line.contains("--uc-min-results=5")
        })
    });
    assert!(
        has_threshold_warning,
        "expected threshold warning in open_uncertainty, got: {open_uncertainty:?}"
    );

    let evidence = json["evidence"].as_str().unwrap_or("");
    assert!(
        evidence.contains("[uc-low-confidence-fallback] semantic fact one"),
        "expected UC evidence line, got: {evidence}"
    );
}

/// End-to-end test: `layers gate --skip-mcp` runs all five pipeline checks
/// against the openclaw-pm workspace.
#[test]
fn test_layers_gate_against_openclaw_pm() {
    let openclaw_pm = Path::new("/Users/bri/Documents/GitHub/openclaw-pm");
    let home = make_home();
    if !seed_openclaw_config(home.path()) {
        eprintln!("test_layers_gate_against_openclaw_pm: skipped (no local config)");
        return;
    }
    let cargo_wrapper = write_cargo_audit_wrapper(home.path());
    let cargo_target_dir = tempfile::tempdir().expect("failed to create cargo target dir");
    let original_path = std::env::var("PATH").unwrap_or_default();
    let wrapper_dir = cargo_wrapper
        .parent()
        .expect("cargo wrapper should have a parent");

    assert!(
        openclaw_pm.exists(),
        "openclaw-pm workspace not found at {}",
        openclaw_pm.display()
    );

    let output = Command::new(layers_bin())
        .current_dir(repo_root())
        .env("HOME", home.path())
        .env("PATH", format!("{}:{original_path}", wrapper_dir.display()))
        .env("CARGO_TARGET_DIR", cargo_target_dir.path())
        .args(["gate", "--skip-mcp", "--workspace"])
        .arg(openclaw_pm)
        .output()
        .expect("failed to execute layers gate");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "layers gate should pass for openclaw-pm.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let combined = format!("{stdout}\n{stderr}");
    for check in &["Format", "Compile", "Clippy", "Tests", "Audit"] {
        assert!(
            combined.contains(check),
            "expected '{check}' check to run. output:\n{combined}"
        );
        assert!(
            combined.contains(&format!("+ {check} passed"))
                || combined.contains(&format!("{check} passed")),
            "expected '{check}' to pass. output:\n{combined}"
        );
    }

    assert!(
        combined.contains("Gate Open"),
        "expected 'Gate Open' success message. output:\n{combined}"
    );
}
