//! Internal skill definitions, embedded from editable TOML for fallback use.
use super::{LoadedSkill, SkillFormat, SkillMeta};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

pub const DEFAULT_SKILLS: &str = include_str!("../../../../skills-internal.toml");

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundledSkill {
    #[serde(skip)]
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub argument_hint: Option<String>,
    #[serde(rename = "prompt")]
    pub prompt_template: String,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default = "yes")]
    pub user_invocable: bool,
    #[serde(default = "yes")]
    pub model_invocable: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    skills: BTreeMap<String, BundledSkill>,
}

pub fn parse_catalog(text: &str) -> Result<Vec<BundledSkill>, String> {
    let doc: Document = toml::from_str(text).map_err(|e| e.to_string())?;
    if doc.version != 1 {
        return Err(format!(
            "Unsupported skills version {}; expected 1",
            doc.version
        ));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (name, mut skill) in doc.skills {
        skill.name = name.to_lowercase();
        for name in std::iter::once(&skill.name).chain(skill.aliases.iter()) {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || ["list", "reload", "path", "paths"].contains(&name.to_lowercase().as_str())
                || !seen.insert(name.to_lowercase())
            {
                return Err(format!(
                    "Invalid, reserved, or duplicate skill name/alias: {name}"
                ));
            }
        }
        if skill.description.trim().is_empty() || skill.prompt_template.trim().is_empty() {
            return Err(format!(
                "Skill '{}' needs a nonempty description and prompt",
                skill.name
            ));
        }
        if skill.enabled {
            result.push(skill);
        }
    }
    Ok(result)
}

pub static BUNDLED_SKILLS: Lazy<Vec<BundledSkill>> =
    Lazy::new(|| parse_catalog(DEFAULT_SKILLS).expect("valid embedded skills"));

pub fn find_bundled_skill(name: &str) -> Option<&'static BundledSkill> {
    BUNDLED_SKILLS.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name) || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })
}

pub fn user_invocable_skills() -> Vec<&'static BundledSkill> {
    BUNDLED_SKILLS.iter().filter(|s| s.user_invocable).collect()
}

pub fn load_bundled(skill: &BundledSkill, _args: Option<&str>) -> LoadedSkill {
    LoadedSkill {
        meta: SkillMeta {
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: None,
            bundled: true,
            aliases: skill.aliases.clone(),
            allowed_tools: skill.allowed_tools.clone(),
            argument_hint: skill.argument_hint.clone(),
            format: SkillFormat::Bundled,
            user_invocable: skill.user_invocable,
            model_invocable: skill.model_invocable,
        },
        content: skill.prompt_template.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_by_name() {
        assert!(find_bundled_skill("simplify").is_some());
        assert!(find_bundled_skill("debug").is_some());
        assert!(find_bundled_skill("commit").is_some());
    }

    #[test]
    fn test_find_by_alias() {
        assert!(find_bundled_skill("mem").is_some());
        assert_eq!(find_bundled_skill("mem").unwrap().name, "remember");
        assert!(find_bundled_skill("diagnose").is_some());
        assert_eq!(find_bundled_skill("diagnose").unwrap().name, "debug");
        assert!(find_bundled_skill("help-me").is_some());
    }

    #[test]
    fn test_case_insensitive() {
        assert!(find_bundled_skill("SIMPLIFY").is_some());
        assert!(find_bundled_skill("Debug").is_some());
    }

    #[test]
    fn test_not_found() {
        assert!(find_bundled_skill("nonexistent").is_none());
    }

    #[test]
    fn test_user_invocable() {
        let invocable = user_invocable_skills();
        assert!(invocable.len() >= 7);
        assert!(invocable.iter().all(|s| s.user_invocable));
    }

    #[test]
    fn test_load_bundled_expand() {
        let skill = find_bundled_skill("debug").unwrap();
        let loaded = load_bundled(skill, Some("the tests are flaky"));
        let expanded = loaded.expand(Some("the tests are flaky"));
        assert!(expanded.contains("the tests are flaky"));
        assert!(!expanded.contains("$ARGUMENTS"));
    }
}
