//! Best-effort dual-write of layers events to the blob store.
//!
//! Content types: layers/audit, layers/curated-memory, council/plan, council/trace, council/learning.

use std::collections::BTreeMap;

pub use substrate::blob::{BlobEnvelope, BlobStore, ContentType, ProducerId};

/// Best-effort put to the blob store. Failures are logged but never propagate.
pub fn put(
    content_type: &str,
    producer: &str,
    payload: Vec<u8>,
    preview: Option<String>,
    extra: BTreeMap<String, String>,
) {
    if let Err(e) = try_put(content_type, producer, payload, preview, extra) {
        eprintln!("warning: blob dual-write failed: {e}");
    }
}

fn try_put(
    content_type: &str,
    producer: &str,
    payload: Vec<u8>,
    preview: Option<String>,
    extra: BTreeMap<String, String>,
) -> Result<(), String> {
    let home = dirs::home_dir().ok_or_else(|| "could not determine home directory".to_string())?;
    let root = home.join(".openclaw").join("blobs");

    let mut store = BlobStore::open(&root).map_err(|e| format!("{e}"))?;

    let ct = ContentType::new(content_type.to_string()).map_err(|e| format!("{e}"))?;
    let prod = ProducerId::new(producer.to_string()).map_err(|e| format!("{e}"))?;

    let envelope = BlobEnvelope::new(ct, prod, payload, preview, extra);
    store.put(&envelope).map_err(|e| format!("{e}"))?;
    Ok(())
}

/// Returns the blob store root path.
#[allow(dead_code)]
pub fn blob_root() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    home.join(".openclaw").join("blobs")
}
