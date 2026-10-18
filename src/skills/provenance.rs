use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const DIGEST_SCOPE: &str = "skill-directory-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceRecord {
    pub name: String,
    pub version: String,
    pub source: String,
    pub digest: String,
    pub digest_scope: String,
}

pub fn compute_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn compute_bytes_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn compute_dir_digest(dir: &Path) -> Result<String, std::io::Error> {
    let mut file_entries: Vec<(String, String)> = Vec::new();
    collect_files_recursive(dir, dir, &mut file_entries)?;
    file_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_path, hash) in file_entries {
        hasher.update(format!("{rel_path}:{hash}\n").as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, String)>,
) -> Result<(), std::io::Error> {
    if !current.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let hash = compute_file_sha256(&path)?;
            out.push((rel, hash));
        }
    }
    Ok(())
}

pub fn build_provenance(
    name: &str,
    dir: &Path,
    version: &str,
    source: &str,
) -> Result<ProvenanceRecord, std::io::Error> {
    let digest = compute_dir_digest(dir)?;
    Ok(ProvenanceRecord {
        name: name.to_string(),
        version: version.to_string(),
        source: source.to_string(),
        digest,
        digest_scope: DIGEST_SCOPE.to_string(),
    })
}
