use crate::skills::embed::EmbeddedSkills;
use crate::skills::paths::{
    AgentTarget, all_agent_targets, canonical_global_skills_dir, canonical_project_skills_dir,
    detect_existing_agent_targets, mycqu_config_dir,
};
use crate::skills::provenance::{build_provenance, compute_dir_digest};
use crate::skills::state::{ManagedSkillRecord, SkillState, read_state, write_state};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetupOptions {
    pub mode: String,
    pub target: String,
    pub global: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub source: Option<PathBuf>,
    pub force: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupPlanItem {
    pub original_path: String,
    pub backup_path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetLinkPlanItem {
    pub target_id: String,
    pub path: String,
    pub points_to: String,
    pub mode: String, // "symlink" or "copy"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillInstallPlan {
    pub skill_name: String,
    pub canonical_path: String,
    pub file_count: usize,
    pub links: Vec<TargetLinkPlanItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetupPlan {
    pub mode: String,
    pub source: String,
    pub canonical_root: String,
    pub skills: Vec<SkillInstallPlan>,
    pub backups: Vec<BackupPlanItem>,
    pub removals: Vec<String>,
    pub is_dry_run: bool,
}

pub fn build_setup_plan(options: &SetupOptions, home: &Path) -> Result<SetupPlan, std::io::Error> {
    let mode = options.mode.as_str();
    if mode != "multi" && mode != "mono" {
        return Err(std::io::Error::other(format!(
            "Invalid mode '{mode}': must be 'multi' or 'mono'"
        )));
    }

    let canonical_root = if options.global {
        canonical_global_skills_dir(home)
    } else {
        canonical_project_skills_dir()
    };

    let config_dir = mycqu_config_dir(home);
    let previous_state = read_state(&config_dir)?;

    let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_dir = config_dir.join("skill-backups").join(&timestamp);

    let mut backups = Vec::new();
    let mut removals = Vec::new();

    // Mutual exclusion cleanup between mono and multi
    if let Some(prev) = previous_state.as_ref().filter(|p| p.mode != mode) {
        let all_targets = all_agent_targets();
        for managed in &prev.managed_skills {
            let old_canonical = canonical_root.join(&managed.name);
            if old_canonical.symlink_metadata().is_ok() {
                backups.push(BackupPlanItem {
                    original_path: old_canonical.display().to_string(),
                    backup_path: backup_dir.join(&managed.name).display().to_string(),
                    reason: "opposite layout cleanup".to_string(),
                });
                removals.push(old_canonical.display().to_string());
            }

            for target_id in &managed.targets {
                if let Some(t_def) = all_targets.iter().find(|t| &t.id == target_id) {
                    let old_target_path = home.join(&t_def.rel_path).join(&managed.name);
                    if old_target_path.symlink_metadata().is_ok() {
                        backups.push(BackupPlanItem {
                            original_path: old_target_path.display().to_string(),
                            backup_path: backup_dir
                                .join(target_id)
                                .join(&managed.name)
                                .display()
                                .to_string(),
                            reason: "opposite layout agent cleanup".to_string(),
                        });
                        removals.push(old_target_path.display().to_string());
                    }
                }
            }
        }
    }

    // Determine skills to install
    let skill_names = if let Some(src_dir) = &options.source {
        if !src_dir.exists() || !src_dir.is_dir() {
            return Err(std::io::Error::other(format!(
                "Local source path does not exist or is not a directory: {}",
                src_dir.display()
            )));
        }
        let mode_sub = src_dir.join(mode);
        let scan_dir = if mode_sub.is_dir() {
            &mode_sub
        } else {
            src_dir
        };
        let mut list = Vec::new();
        for entry in fs::read_dir(scan_dir)? {
            let entry = entry?;
            let path = entry.path();
            let is_skill = path.is_dir() && path.join("SKILL.md").exists();
            if let (true, Some(name)) = (is_skill, path.file_name().and_then(|s| s.to_str())) {
                list.push(name.to_string());
            }
        }
        if list.is_empty() {
            return Err(std::io::Error::other(format!(
                "No valid skills found in local source directory '{}' for mode '{}'",
                src_dir.display(),
                mode
            )));
        }
        list
    } else {
        EmbeddedSkills::list_skills(mode)
    };

    // Determine target agent directories
    let active_targets: Vec<AgentTarget> = if options.target == "all" {
        detect_existing_agent_targets(home)
    } else if options.target == "agents" {
        Vec::new() // only canonical
    } else {
        all_agent_targets()
            .into_iter()
            .filter(|t| t.id == options.target)
            .collect()
    };

    let source_desc = match &options.source {
        Some(p) => format!("local: {}", p.display()),
        None => "embedded".to_string(),
    };

    let mut skill_plans = Vec::new();

    for name in &skill_names {
        let dest = canonical_root.join(name);
        if dest.exists() {
            let mut should_backup = options.force;
            if !should_backup {
                if let Some(prev) = &previous_state {
                    if let Some(managed) = prev.managed_skills.iter().find(|m| &m.name == name) {
                        match compute_dir_digest(&dest) {
                            Ok(current_digest) => {
                                if current_digest != managed.digest {
                                    should_backup = true;
                                }
                            }
                            Err(_) => {
                                should_backup = true;
                            }
                        }
                    } else {
                        should_backup = true;
                    }
                } else {
                    should_backup = true;
                }
            }

            if should_backup {
                backups.push(BackupPlanItem {
                    original_path: dest.display().to_string(),
                    backup_path: backup_dir.join(name).display().to_string(),
                    reason: "overwrite existing canonical skill".to_string(),
                });
            }
        }

        let file_count = if let Some(src_dir) = &options.source {
            count_files_in_dir(&src_dir.join(mode).join(name))
                .or_else(|_| count_files_in_dir(&src_dir.join(name)))
                .unwrap_or(0)
        } else {
            EmbeddedSkills::get_skill_files(mode, name).len()
        };

        let mut links = Vec::new();
        for target in &active_targets {
            let target_skill_dir = home.join(&target.rel_path).join(name);
            if target_skill_dir.symlink_metadata().is_ok() {
                let is_already_symlinked = fs::read_link(&target_skill_dir)
                    .map(|tgt| tgt == dest)
                    .unwrap_or(false);
                if !is_already_symlinked {
                    backups.push(BackupPlanItem {
                        original_path: target_skill_dir.display().to_string(),
                        backup_path: backup_dir.join(&target.id).join(name).display().to_string(),
                        reason: "overwrite existing agent target skill".to_string(),
                    });
                }
            }
            let link_mode = determine_link_mode(&dest, &target_skill_dir);
            links.push(TargetLinkPlanItem {
                target_id: target.id.clone(),
                path: target_skill_dir.display().to_string(),
                points_to: dest.display().to_string(),
                mode: link_mode,
            });
        }

        skill_plans.push(SkillInstallPlan {
            skill_name: name.clone(),
            canonical_path: dest.display().to_string(),
            file_count,
            links,
        });
    }

    Ok(SetupPlan {
        mode: mode.to_string(),
        source: source_desc,
        canonical_root: canonical_root.display().to_string(),
        skills: skill_plans,
        backups,
        removals,
        is_dry_run: options.dry_run,
    })
}

fn determine_link_mode(_src: &Path, _dest: &Path) -> String {
    #[cfg(unix)]
    {
        "symlink".to_string()
    }
    #[cfg(windows)]
    {
        // On Windows, test if creating symlink works, otherwise copy
        "copy".to_string()
    }
}

fn count_files_in_dir(dir: &Path) -> Result<usize, std::io::Error> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            count += count_files_in_dir(&p)?;
        } else {
            count += 1;
        }
    }
    Ok(count)
}

pub fn execute_setup_plan(
    plan: &SetupPlan,
    options: &SetupOptions,
    home: &Path,
) -> Result<SkillState, std::io::Error> {
    if plan.is_dry_run {
        return Err(std::io::Error::other("Cannot execute dry-run plan"));
    }

    let config_dir = mycqu_config_dir(home);
    let canonical_root = PathBuf::from(&plan.canonical_root);
    fs::create_dir_all(&canonical_root)?;

    // 1. Perform backups
    for b in &plan.backups {
        let orig = PathBuf::from(&b.original_path);
        let dest = PathBuf::from(&b.backup_path);
        if orig.exists() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if let Err(e) = copy_dir_all(&orig, &dest) {
                return Err(std::io::Error::other(format!(
                    "Failed to backup {}: {e}",
                    orig.display()
                )));
            }
        }
    }

    // 2. Perform removals (e.g. mutual layout removals)
    for rem in &plan.removals {
        let p = PathBuf::from(rem);
        if p.symlink_metadata().is_ok() {
            safe_remove_path(&p)?;
        }
    }

    let mut managed_records = Vec::new();
    let mut official_skills = Vec::new();

    // 3. Install each skill to canonical path atomically
    for skill in &plan.skills {
        let name = &skill.skill_name;
        let dest = canonical_root.join(name);

        let staging_dir = canonical_root.join(format!(".tmp_{name}_{}", std::process::id()));
        if staging_dir.exists() {
            let _ = fs::remove_dir_all(&staging_dir);
        }
        fs::create_dir_all(&staging_dir)?;

        if let Some(src_dir) = &options.source {
            let candidate1 = src_dir.join(&plan.mode).join(name);
            let candidate2 = src_dir.join(name);
            let actual_src = if candidate1.is_dir() {
                candidate1
            } else {
                candidate2
            };
            copy_dir_all(&actual_src, &staging_dir)?;
        } else {
            let embedded_files = EmbeddedSkills::get_skill_files(&plan.mode, name);
            for file in embedded_files {
                let out_file = staging_dir.join(&file.rel_path);
                if let Some(parent) = out_file.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_file, &file.data)?;
            }
        }

        // Atomic publish to canonical
        if dest.symlink_metadata().is_ok() {
            safe_remove_path(&dest)?;
        }
        fs::rename(&staging_dir, &dest)?;

        let prov = build_provenance(name, &dest, "0.1.0", &plan.source)?;
        let mut target_ids = Vec::new();

        // 4. Link or copy to agent targets
        let mut chosen_link_mode = "symlink".to_string();
        for link in &skill.links {
            let target_path = PathBuf::from(&link.path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if target_path.symlink_metadata().is_ok() {
                safe_remove_path(&target_path)?;
            }

            // Try symlink first, fallback to copy
            let symlink_res = create_dir_symlink(&dest, &target_path);
            if symlink_res.is_err() {
                chosen_link_mode = "copy".to_string();
                copy_dir_all(&dest, &target_path)?;
            }
            target_ids.push(link.target_id.clone());
        }

        // Retain any targets that were previously managed and still exist on disk
        let previous = read_state(&config_dir)?;
        if let Some(old_rec) = previous
            .as_ref()
            .and_then(|prev| prev.managed_skills.iter().find(|m| &m.name == name))
        {
            let all_targets = all_agent_targets();
            for old_target in &old_rec.targets {
                if target_ids.contains(old_target) {
                    continue;
                }
                if let Some(target) = all_targets.iter().find(|t| &t.id == old_target) {
                    let path = home.join(&target.rel_path).join(name);
                    if path.symlink_metadata().is_ok() {
                        target_ids.push(old_target.clone());
                    }
                }
            }
        }
        managed_records.push(ManagedSkillRecord::from((
            prov,
            target_ids,
            chosen_link_mode,
        )));
        official_skills.push(name.clone());
    }

    let new_state = SkillState {
        version: "0.1.0".to_string(),
        mode: plan.mode.clone(),
        official_skills,
        managed_skills: managed_records,
        updated_at: Local::now().to_rfc3339(),
    };

    write_state(&config_dir, &new_state)?;

    Ok(new_state)
}

fn create_dir_symlink(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(src, dst)
    }
}

pub fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
pub fn safe_remove_path(p: &Path) -> Result<(), std::io::Error> {
    let meta = match p.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if meta.file_type().is_symlink_dir() {
                std::fs::remove_dir(p)
            } else {
                std::fs::remove_file(p)
            }
        }
        #[cfg(unix)]
        {
            std::fs::remove_file(p)
        }
    } else if meta.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_targets_retains_ownership_and_switching_modes_removes_old_targets() {
        let home = std::env::temp_dir().join(crate::challenge::generate_challenge_id());
        fs::create_dir_all(&home).unwrap();
        let mut options = SetupOptions {
            mode: "multi".into(),
            target: "claude".into(),
            global: true,
            dry_run: false,
            yes: true,
            source: None,
            force: false,
        };
        let plan = build_setup_plan(&options, &home).unwrap();
        execute_setup_plan(&plan, &options, &home).unwrap();
        options.target = "cursor".into();
        let plan = build_setup_plan(&options, &home).unwrap();
        let state = execute_setup_plan(&plan, &options, &home).unwrap();
        for record in &state.managed_skills {
            assert!(record.targets.iter().any(|t| t == "claude"));
            assert!(record.targets.iter().any(|t| t == "cursor"));
        }
        options.mode = "mono".into();
        let plan = build_setup_plan(&options, &home).unwrap();
        execute_setup_plan(&plan, &options, &home).unwrap();
        for target in [".claude", ".cursor"] {
            assert!(
                home.join(target)
                    .join("skills/cqu-reverse")
                    .symlink_metadata()
                    .is_err()
            );
        }
        assert!(home.join(".agents/skills/mycqu/SKILL.md").is_file());
        let canonical = home.join(".agents/skills/mycqu");
        safe_remove_path(&canonical).unwrap();
        let target = home.join(".cursor/skills/mycqu");
        safe_remove_path(&target).unwrap();
        assert!(target.symlink_metadata().is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn test_rejects_nonexistent_source() {
        let temp = std::env::temp_dir().join(format!("mycqu-test-src-{}", std::process::id()));
        let opts = SetupOptions {
            mode: "multi".to_string(),
            target: "all".to_string(),
            global: true,
            dry_run: true,
            yes: true,
            source: Some(temp.join("nonexistent_dir")),
            force: false,
        };
        let res = build_setup_plan(&opts, &temp);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_mono_mode_plan_includes_mycqu() {
        let temp = std::env::temp_dir().join(format!("mycqu-test-mono-{}", std::process::id()));
        let opts = SetupOptions {
            mode: "mono".to_string(),
            target: "agents".to_string(),
            global: true,
            dry_run: true,
            yes: true,
            source: None,
            force: false,
        };
        let plan = build_setup_plan(&opts, &temp).expect("plan builds");
        assert_eq!(plan.mode, "mono");
        assert!(plan.skills.iter().any(|s| s.skill_name == "mycqu"));
    }
}
