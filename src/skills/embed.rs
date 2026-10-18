use rust_embed::RustEmbed;
use std::borrow::Cow;
use std::collections::BTreeSet;

#[derive(RustEmbed)]
#[folder = "skills/"]
pub struct EmbeddedSkills;

#[derive(Clone, Debug)]
pub struct EmbeddedFile {
    pub rel_path: String,
    pub data: Cow<'static, [u8]>,
}

impl EmbeddedSkills {
    pub fn get_skill_files(mode: &str, skill_name: &str) -> Vec<EmbeddedFile> {
        let prefix = format!("{mode}/{skill_name}/");
        let mut files = Vec::new();
        for file in Self::iter() {
            let path_str = file.as_ref();
            if let (Some(stripped), Some(content)) =
                (path_str.strip_prefix(&prefix), Self::get(path_str))
            {
                files.push(EmbeddedFile {
                    rel_path: stripped.replace('\\', "/"),
                    data: content.data,
                });
            }
        }
        files
    }

    pub fn list_skills(mode: &str) -> Vec<String> {
        let prefix = format!("{mode}/");
        let mut names = BTreeSet::new();
        for file in Self::iter() {
            let path_str = file.as_ref();
            if let Some(first) = path_str
                .strip_prefix(&prefix)
                .and_then(|s| s.split('/').next())
                .filter(|s| !s.is_empty())
            {
                names.insert(first.to_string());
            }
        }
        names.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_skills_listing() {
        let multi_skills = EmbeddedSkills::list_skills("multi");
        assert!(multi_skills.contains(&"mycqu".to_string()));
        assert!(multi_skills.contains(&"cqu-reverse".to_string()));

        let files = EmbeddedSkills::get_skill_files("multi", "mycqu");
        assert!(files.iter().any(|f| f.rel_path == "SKILL.md"));
        assert!(files.iter().any(|f| f.rel_path.starts_with("references/")));

        let mono_skills = EmbeddedSkills::list_skills("mono");
        assert!(mono_skills.contains(&"mycqu".to_string()));
        let mono_files = EmbeddedSkills::get_skill_files("mono", "mycqu");
        assert!(mono_files.iter().any(|f| f.rel_path == "SKILL.md"));
    }
}
