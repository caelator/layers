use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::SecondsFormat;

use super::types::{FeatureVerdict, ProofRecord};

/// Best-effort dual-write to blob store (`substrate::blob`).
/// Failures are logged but never propagate — the primary write always succeeds.
fn blob_dual_write(
    content_type: &str,
    key: &str,
    data: &[u8],
    preview: &str,
    tags: &[(&str, &str)],
) {
    if let Err(e) = try_blob_put(content_type, key, data, preview, tags) {
        eprintln!("warning: blob dual-write failed: {e}");
    }
}

fn try_blob_put(
    content_type: &str,
    _key: &str,
    data: &[u8],
    preview: &str,
    tags: &[(&str, &str)],
) -> Result<(), String> {
    let home = dirs::home_dir().ok_or_else(|| "could not determine home directory".to_string())?;
    let root = home.join(".openclaw").join("blobs");

    let mut store = substrate::blob::BlobStore::open(&root).map_err(|e| format!("{e}"))?;

    let ct =
        substrate::blob::ContentType::new(content_type.to_string()).map_err(|e| format!("{e}"))?;
    let producer =
        substrate::blob::ProducerId::new("maestro".to_string()).map_err(|e| format!("{e}"))?;
    let timestamp = substrate::blob::now_timestamp_ms();

    let mut extra = std::collections::BTreeMap::new();
    for (k, v) in tags {
        extra.insert(k.to_string(), v.to_string());
    }

    let envelope = substrate::blob::BlobEnvelope::new(
        ct,
        producer,
        data.to_vec(),
        Some(preview.to_string()),
        extra,
    );
    store.put(&envelope).map_err(|e| format!("{e}"))?;

    let _ = timestamp; // suppress unused warning
    Ok(())
}

pub struct ArtifactStore {
    workspace_root: PathBuf,
}

impl ArtifactStore {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    pub fn write_proof(&self, record: &ProofRecord) -> Result<PathBuf> {
        let directory = self
            .workspace_root
            .join(".proveit")
            .join("artifacts")
            .join(&record.feature_id)
            .join(&record.proof_id);
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;

        let timestamp = record
            .timestamp
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            .replace(':', "-");
        let short_sha: String = record.commit_sha.chars().take(12).collect();
        let path = directory.join(format!("{timestamp}_{short_sha}.json"));
        let payload = serde_json::to_string_pretty(record)?;
        fs::write(&path, &payload)
            .with_context(|| format!("failed to write {}", path.display()))?;

        // Best-effort dual-write to blob store
        let preview = format!(
            "{}:{} {} ({})",
            record.feature_id,
            record.proof_id,
            if record.passed { "PASS" } else { "FAIL" },
            record.commit_sha.chars().take(8).collect::<String>(),
        );
        blob_dual_write(
            "proveit/artifact",
            &record.proof_id,
            payload.as_bytes(),
            &preview,
            &[
                ("feature_id", record.feature_id.as_str()),
                ("proof_id", record.proof_id.as_str()),
                ("passed", if record.passed { "true" } else { "false" }),
                ("commit_sha", &record.commit_sha),
            ],
        );

        Ok(path)
    }

    pub fn latest_proof(&self, feature_id: &str, proof_id: &str) -> Result<Option<ProofRecord>> {
        let directory = self
            .workspace_root
            .join(".proveit")
            .join("artifacts")
            .join(feature_id)
            .join(proof_id);
        if !directory.exists() {
            return Ok(None);
        }

        let mut candidates = Vec::new();
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                candidates.push(path);
            }
        }
        candidates.sort();
        candidates.reverse();

        if let Some(path) = candidates.into_iter().next() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let record = serde_json::from_str::<ProofRecord>(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            return Ok(Some(record));
        }

        Ok(None)
    }

    pub fn write_report(&self, feature_id: &str, verdict: &FeatureVerdict) -> Result<PathBuf> {
        let directory = self.workspace_root.join(".proveit").join("verdicts");
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let path = directory.join(format!("{feature_id}.json"));
        let payload = serde_json::to_string_pretty(verdict)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }
}
