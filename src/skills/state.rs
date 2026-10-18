use crate::skills::provenance::ProvenanceRecord;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const STATE_FILE_NAME: &str = "skills-state.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedSkillRecord {
    pub name: String,
    pub version: String,
    pub source: String,
    pub digest: String,
    pub digest_scope: String,
    pub targets: Vec<String>,
    #[serde(default)]
    pub link_mode: String,
}

impl From<(ProvenanceRecord, Vec<String>, String)> for ManagedSkillRecord {
    fn from((prov, targets, link_mode): (ProvenanceRecord, Vec<String>, String)) -> Self {
        Self {
            name: prov.name,
            version: prov.version,
            source: prov.source,
            digest: prov.digest,
            digest_scope: prov.digest_scope,
            targets,
            link_mode,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillState {
    pub version: String,
    pub mode: String,
    pub official_skills: Vec<String>,
    pub managed_skills: Vec<ManagedSkillRecord>,
    pub updated_at: String,
}

pub fn state_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE_NAME)
}

pub fn read_state(config_dir: &Path) -> Result<Option<SkillState>, std::io::Error> {
    let p = state_file_path(config_dir);
    if !p.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&p)?;
    let state = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::other(format!("Invalid skills-state.json: {e}")))?;
    Ok(Some(state))
}

pub fn write_state(config_dir: &Path, state: &SkillState) -> Result<(), std::io::Error> {
    fs::create_dir_all(config_dir)?;
    let p = state_file_path(config_dir);
    let tmp_path = config_dir.join(format!("{STATE_FILE_NAME}.tmp.{}", std::process::id()));
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::other(format!("Serialize error: {e}")))?;
    fs::write(&tmp_path, json)?;
    fs::rename(&tmp_path, &p)?;
    Ok(())
}
