use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::types::FeatureManifest;

pub fn manifests_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".proveit").join("manifests")
}

pub fn load_manifest(workspace_root: &Path, feature_id: &str) -> Result<FeatureManifest> {
    let path = manifests_dir(workspace_root).join(format!("{feature_id}.toml"));
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let manifest = toml::from_str::<FeatureManifest>(&raw)
        .with_context(|| format!("failed to parse manifest {}", path.display()))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn load_all_manifests(workspace_root: &Path) -> Result<Vec<FeatureManifest>> {
    let dir = manifests_dir(workspace_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read manifest {}", path.display()))?;
        let manifest = toml::from_str::<FeatureManifest>(&raw)
            .with_context(|| format!("failed to parse manifest {}", path.display()))?;
        validate_manifest(&manifest)?;
        manifests.push(manifest);
    }

    manifests.sort_by(|left, right| left.feature.id.cmp(&right.feature.id));
    Ok(manifests)
}

fn validate_manifest(manifest: &FeatureManifest) -> Result<()> {
    if manifest.feature.id.trim().is_empty() {
        bail!("manifest feature.id must not be empty");
    }
    if manifest.feature.owner.trim().is_empty() {
        bail!("manifest feature.owner must not be empty");
    }
    if manifest.feature.required_score > 5 {
        bail!(
            "manifest {} has invalid required_score {}; max is 5",
            manifest.feature.id,
            manifest.feature.required_score
        );
    }
    if manifest.proofs.is_empty() {
        bail!(
            "manifest {} must define at least one proof",
            manifest.feature.id
        );
    }
    Ok(())
}
