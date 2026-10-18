use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTarget {
    pub id: String,
    pub name: String,
    pub rel_path: PathBuf,
}

pub fn canonical_global_skills_dir(home: &Path) -> PathBuf {
    home.join(".agents").join("skills")
}

pub fn canonical_project_skills_dir() -> PathBuf {
    PathBuf::from(".agents").join("skills")
}

pub fn mycqu_config_dir(home: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("MYCQU_CONFIG_DIR") {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    home.join(".mycqu")
}

pub fn all_agent_targets() -> Vec<AgentTarget> {
    vec![
        AgentTarget {
            id: "claude".to_string(),
            name: "Claude Code".to_string(),
            rel_path: PathBuf::from(".claude").join("skills"),
        },
        AgentTarget {
            id: "cursor".to_string(),
            name: "Cursor".to_string(),
            rel_path: PathBuf::from(".cursor").join("skills"),
        },
        AgentTarget {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            rel_path: PathBuf::from(".codex").join("skills"),
        },
        AgentTarget {
            id: "opencode".to_string(),
            name: "OpenCode".to_string(),
            rel_path: PathBuf::from(".config").join("opencode").join("skills"),
        },
        AgentTarget {
            id: "copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            rel_path: PathBuf::from(".copilot").join("skills"),
        },
        AgentTarget {
            id: "gemini".to_string(),
            name: "Gemini CLI".to_string(),
            rel_path: PathBuf::from(".gemini").join("skills"),
        },
        AgentTarget {
            id: "continue".to_string(),
            name: "Continue".to_string(),
            rel_path: PathBuf::from(".continue").join("skills"),
        },
        AgentTarget {
            id: "windsurf".to_string(),
            name: "Windsurf".to_string(),
            rel_path: PathBuf::from(".codeium").join("windsurf").join("skills"),
        },
        AgentTarget {
            id: "qoder".to_string(),
            name: "Qoder".to_string(),
            rel_path: PathBuf::from(".qoder").join("skills"),
        },
        AgentTarget {
            id: "trae".to_string(),
            name: "Trae".to_string(),
            rel_path: PathBuf::from(".trae").join("skills"),
        },
    ]
}

pub fn detect_existing_agent_targets(home: &Path) -> Vec<AgentTarget> {
    all_agent_targets()
        .into_iter()
        .filter(|target| {
            // Check if the parent agent tool dir exists (e.g. ~/.claude or ~/.cursor)
            let parent = home.join(&target.rel_path);
            if let Some(tool_root) = parent.parent() {
                tool_root.exists()
            } else {
                false
            }
        })
        .collect()
}
