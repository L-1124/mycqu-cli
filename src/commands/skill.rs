use clap::Subcommand;
use serde_json::json;
use std::path::PathBuf;

use crate::output::{
    self, ApiResponse, EXIT_INVALID_ARGUMENT, EXIT_PERMISSION_DENIED, EXIT_STATE_INVALID,
    EXIT_SUCCESS,
};
use crate::skills::embed::EmbeddedSkills;
use crate::skills::paths::{
    all_agent_targets, canonical_global_skills_dir, detect_existing_agent_targets, mycqu_config_dir,
};
use crate::skills::provenance::compute_dir_digest;
use crate::skills::setup::{SetupOptions, build_setup_plan, execute_setup_plan};
use crate::skills::state::{read_state, write_state};

#[derive(Subcommand, Debug, Clone)]
pub enum SkillCommands {
    #[command(about = "安装并分发 Skill 到本机 Agent 环境（Claude Code, Cursor, Codex 等）")]
    Setup {
        #[arg(long, default_value = "multi", value_parser = ["multi", "mono"], help = "布局模式: multi (推荐) 或 mono")]
        mode: String,
        #[arg(
            long,
            default_value = "all",
            help = "目标工具: all, agents, claude, cursor, codex, opencode..."
        )]
        target: String,
        #[arg(long, help = "仅预览安装计划与备份动作，不进行实际磁盘操作")]
        dry_run: bool,
        #[arg(short = 'y', long, help = "跳过交互式确认")]
        yes: bool,
        #[arg(short = 'g', long, default_value_t = true, action = clap::ArgAction::SetTrue, help = "安装到用户全局 ~/.agents/skills（默认）")]
        global: bool,
        #[arg(long, help = "从指定本地文件路径加载 Skill，覆盖内嵌资源")]
        source: Option<PathBuf>,
        #[arg(long, help = "强制重新覆盖安装")]
        force: bool,
    },
    #[command(about = "列出当前可用的内嵌 Skill 以及宿主机的安装状态")]
    List,
    #[command(about = "检查当前已安装 Skill 的完整性与版本摘要")]
    Check,
    #[command(about = "卸载受管的 Skill")]
    Uninstall {
        #[arg(long, help = "要卸载的 Skill 名称；不指定则卸载全部受管 Skill")]
        skill: Option<String>,
        #[arg(short = 'y', long, help = "跳过确认")]
        yes: bool,
    },
}

pub fn handle_skill(cmd: SkillCommands) -> i32 {
    let home = match directories::BaseDirs::new() {
        Some(base) => base.home_dir().to_path_buf(),
        None => {
            output::print_err(
                "INTERNAL_ERROR",
                "Cannot resolve user home directory",
                false,
                None,
                None,
            );
            return EXIT_STATE_INVALID;
        }
    };

    match cmd {
        SkillCommands::Setup {
            mode,
            target,
            dry_run,
            yes: _,
            global,
            source,
            force,
        } => {
            let options = SetupOptions {
                mode,
                target,
                global,
                dry_run,
                yes: true,
                source,
                force,
            };

            let plan = match build_setup_plan(&options, &home) {
                Ok(p) => p,
                Err(e) => {
                    output::print_err(
                        "USAGE_ERROR",
                        &format!("Failed to build setup plan: {e}"),
                        false,
                        None,
                        None,
                    );
                    return EXIT_INVALID_ARGUMENT;
                }
            };

            if dry_run {
                output::print_json(&ApiResponse::success(plan, true, None));
                return EXIT_SUCCESS;
            }

            match execute_setup_plan(&plan, &options, &home) {
                Ok(state) => {
                    output::print_json(&ApiResponse::success(
                        json!({
                            "status": "installed",
                            "state": state,
                            "plan": plan,
                        }),
                        true,
                        None,
                    ));
                    EXIT_SUCCESS
                }
                Err(e) => {
                    output::print_err(
                        "INTERNAL_ERROR",
                        &format!("Failed to execute skill setup: {e}"),
                        false,
                        None,
                        None,
                    );
                    EXIT_STATE_INVALID
                }
            }
        }
        SkillCommands::List => {
            let config_dir = mycqu_config_dir(&home);
            let state = read_state(&config_dir).unwrap_or(None);
            let detected = detect_existing_agent_targets(&home);
            let detected_names: Vec<String> = detected.into_iter().map(|t| t.id).collect();

            let multi_skills = EmbeddedSkills::list_skills("multi");
            let mono_skills = EmbeddedSkills::list_skills("mono");

            let data = json!({
                "embedded": {
                    "multi": multi_skills,
                    "mono": mono_skills,
                },
                "detected_agents": detected_names,
                "installed_state": state,
            });

            output::print_json(&ApiResponse::success(data, true, None));
            EXIT_SUCCESS
        }
        SkillCommands::Check => {
            let config_dir = mycqu_config_dir(&home);
            let state = match read_state(&config_dir) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    output::print_json(&ApiResponse::success(
                        json!({
                            "installed": false,
                            "message": "No skills currently installed or managed by mycqu-cli",
                        }),
                        true,
                        None,
                    ));
                    return EXIT_SUCCESS;
                }
                Err(e) => {
                    output::print_err(
                        "INTERNAL_ERROR",
                        &format!("Failed to read skills-state.json: {e}"),
                        false,
                        None,
                        None,
                    );
                    return EXIT_STATE_INVALID;
                }
            };

            let canonical_root = canonical_global_skills_dir(&home);
            let mut audits = Vec::new();
            let mut has_drift = false;

            for managed in &state.managed_skills {
                let skill_dir = canonical_root.join(&managed.name);
                if !skill_dir.exists() {
                    audits.push(json!({
                        "name": managed.name,
                        "status": "missing",
                        "expected_path": skill_dir.display().to_string(),
                    }));
                    has_drift = true;
                    continue;
                }

                match compute_dir_digest(&skill_dir) {
                    Ok(current_digest) => {
                        let matches = current_digest == managed.digest;
                        if !matches {
                            has_drift = true;
                        }
                        audits.push(json!({
                            "name": managed.name,
                            "status": if matches { "verified" } else { "modified" },
                            "path": skill_dir.display().to_string(),
                            "expected_digest": managed.digest,
                            "actual_digest": current_digest,
                            "targets": managed.targets,
                            "link_mode": managed.link_mode,
                        }));
                    }
                    Err(e) => {
                        has_drift = true;
                        audits.push(json!({
                            "name": managed.name,
                            "status": "unreadable",
                            "error": e.to_string(),
                        }));
                    }
                }
            }

            output::print_json(&ApiResponse::success(
                json!({
                    "installed": true,
                    "version": state.version,
                    "mode": state.mode,
                    "clean": !has_drift,
                    "skills": audits,
                }),
                true,
                None,
            ));
            EXIT_SUCCESS
        }
        SkillCommands::Uninstall { skill, yes: _ } => {
            let config_dir = mycqu_config_dir(&home);
            let mut state = match read_state(&config_dir) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    output::print_err(
                        "NOT_FOUND",
                        "No installed skills found to uninstall",
                        false,
                        None,
                        None,
                    );
                    return EXIT_PERMISSION_DENIED;
                }
                Err(e) => {
                    output::print_err(
                        "INTERNAL_ERROR",
                        &format!("Failed to read skills-state.json: {e}"),
                        false,
                        None,
                        None,
                    );
                    return EXIT_STATE_INVALID;
                }
            };

            let canonical_root = canonical_global_skills_dir(&home);
            let all_targets = all_agent_targets();

            let target_names: Vec<String> = match &skill {
                Some(name) => {
                    let trimmed = name.trim();
                    if trimmed.is_empty()
                        || trimmed.contains('/')
                        || trimmed.contains('\\')
                        || trimmed.contains("..")
                        || trimmed.starts_with('.')
                    {
                        output::print_err(
                            "INVALID_ARGUMENT",
                            &format!(
                                "Invalid skill name: '{name}'. Skill name must be a single directory identifier"
                            ),
                            false,
                            None,
                            None,
                        );
                        return EXIT_INVALID_ARGUMENT;
                    }
                    if !state.managed_skills.iter().any(|m| m.name == trimmed) {
                        output::print_err(
                            "NOT_FOUND",
                            &format!(
                                "Skill '{trimmed}' is not currently managed in skills-state.json"
                            ),
                            false,
                            None,
                            None,
                        );
                        return EXIT_PERMISSION_DENIED;
                    }
                    vec![trimmed.to_string()]
                }
                None => state
                    .managed_skills
                    .iter()
                    .map(|m| m.name.clone())
                    .collect(),
            };

            let mut removed = Vec::new();
            let mut failed = Vec::new();
            let mut successfully_removed_names = Vec::new();

            for name in &target_names {
                let mut skill_removal_ok = true;
                let canonical_path = canonical_root.join(name);
                if canonical_path.exists() {
                    match std::fs::remove_dir_all(&canonical_path) {
                        Ok(_) => removed.push(canonical_path.display().to_string()),
                        Err(e) => {
                            failed.push(format!("{}: {e}", canonical_path.display()));
                            skill_removal_ok = false;
                        }
                    }
                }

                if let Some(record) = state.managed_skills.iter().find(|m| &m.name == name) {
                    for target_id in &record.targets {
                        if let Some(target_def) = all_targets.iter().find(|t| &t.id == target_id) {
                            let p = home.join(&target_def.rel_path).join(name);
                            if p.symlink_metadata().is_ok() {
                                match crate::skills::setup::safe_remove_path(&p) {
                                    Ok(_) => removed.push(p.display().to_string()),
                                    Err(e) => {
                                        failed.push(format!("{}: {e}", p.display()));
                                        skill_removal_ok = false;
                                    }
                                }
                            }
                        }
                    }
                }

                if skill_removal_ok {
                    successfully_removed_names.push(name.clone());
                }
            }

            state
                .managed_skills
                .retain(|m| !successfully_removed_names.contains(&m.name));
            state
                .official_skills
                .retain(|o| !successfully_removed_names.contains(o));

            if let Err(e) = write_state(&config_dir, &state) {
                output::print_err(
                    "INTERNAL_ERROR",
                    &format!("Failed to update skills-state.json after uninstall: {e}"),
                    false,
                    None,
                    None,
                );
                return EXIT_STATE_INVALID;
            }

            if !failed.is_empty() {
                output::print_err(
                    "INTERNAL_ERROR",
                    &format!(
                        "Failed to remove one or more skill directories: {}",
                        failed.join("; ")
                    ),
                    false,
                    None,
                    None,
                );
                return EXIT_STATE_INVALID;
            }

            output::print_json(&ApiResponse::success(
                json!({
                    "status": "uninstalled",
                    "removed_paths": removed,
                }),
                true,
                None,
            ));
            EXIT_SUCCESS
        }
    }
}
