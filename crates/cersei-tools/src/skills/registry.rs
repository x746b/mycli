//! One catalog shared by user commands and model tool calls.
use super::{bundled, discovery, LoadedSkill, SkillMeta};
use std::path::{Path, PathBuf};

pub struct Registry {
    internal: Vec<bundled::BundledSkill>,
    pub skills: Vec<SkillMeta>,
}

impl Registry {
    pub fn new(internal: Vec<bundled::BundledSkill>, root: &Path, paths: &[PathBuf]) -> Self {
        let skills = discovery::discover_with_bundled(Some(root), paths, &internal);
        Self { internal, skills }
    }

    pub fn list(&self, user: bool) -> Vec<SkillMeta> {
        self.skills
            .iter()
            .filter(|s| {
                if user {
                    s.user_invocable
                } else {
                    s.model_invocable
                }
            })
            .cloned()
            .collect()
    }

    pub fn load(&self, name: &str, user: bool) -> Result<LoadedSkill, String> {
        let meta = self
            .skills
            .iter()
            .find(|s| {
                s.name.eq_ignore_ascii_case(name)
                    || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| format!("Skill '{name}' not found; use /skill list"))?;
        if !(if user {
            meta.user_invocable
        } else {
            meta.model_invocable
        }) {
            return Err(format!(
                "Skill '{}' cannot be invoked by {}",
                meta.name,
                if user { "the user" } else { "the model" }
            ));
        }
        discovery::load_discovered(meta, &self.internal)
    }
}

/// Supply the base directory so the model can resolve references and scripts.
pub fn invocation(skill: &LoadedSkill, args: Option<&str>) -> String {
    let base = skill
        .meta
        .path
        .as_ref()
        .and_then(|p| Path::new(p).parent())
        .map(|p| {
            format!(
                "\nSkill directory: {}\nResolve relative references from this directory.\n",
                p.display()
            )
        })
        .unwrap_or_default();
    format!(
        "# Skill: {}\n{}\n{}",
        skill.meta.name,
        base,
        skill.expand(args)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_name_metadata_arguments_and_references_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join(".claude/skills/different-folder");
        std::fs::create_dir_all(folder.join("references")).unwrap();
        std::fs::write(folder.join("references/details.md"), "Supporting document").unwrap();
        std::fs::write(folder.join("SKILL.md"), "---\nname: 'Review-Thing'\ndescription: >\n  Review the\n  current thing\nargument-hint: '<target>'\nallowed-tools: [Read, Grep]\ndisable-model-invocation: true\n---\nInspect $ARGUMENTS in ${CLAUDE_SKILL_DIR}; first=$0 and $ARGUMENTS[1]").unwrap();
        let registry = Registry::new(vec![], root.path(), &[]);
        let skill = registry.load("review-thing", true).unwrap();
        assert_eq!(skill.meta.description, "Review the current thing");
        assert_eq!(skill.meta.argument_hint.as_deref(), Some("<target>"));
        assert_eq!(
            skill.meta.allowed_tools.as_ref().unwrap(),
            &["Read", "Grep"]
        );
        assert!(registry.load("review-thing", false).is_err());
        assert!(!registry
            .list(false)
            .iter()
            .any(|s| s.name == "Review-Thing"));
        assert!(!registry.skills.iter().any(|s| s.name.contains("details")));
        let result = invocation(&skill, Some("foo $ARGUMENTS"));
        assert!(result.contains("Inspect foo $ARGUMENTS in"));
        assert!(result.contains(&folder.display().to_string()));
        assert!(result.contains("first=foo and $ARGUMENTS"));
    }

    #[test]
    fn nested_commands_and_custom_directories_are_loadable() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join(".claude/commands/team");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("review.md"), "Review $ARGUMENTS").unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(
            external.path().join("SKILL.md"),
            "---\nname: extra\nuser-invocable: false\n---\nModel-only instruction",
        )
        .unwrap();
        let registry = Registry::new(vec![], root.path(), &[external.path().to_owned()]);
        assert!(registry
            .load("TEAM:REVIEW", true)
            .unwrap()
            .expand(Some("code"))
            .contains("Review code"));
        assert!(registry.load("extra", false).is_ok());
        assert!(registry.load("extra", true).is_err());
        assert!(registry.load("../../etc/passwd", true).is_err());
    }

    #[test]
    fn internal_catalog_can_replace_remove_and_validate_skills() {
        let root = tempfile::tempdir().unwrap();
        let custom = bundled::parse_catalog("version=1\n[skills.greet]\ndescription='Greet'\naliases=['hi']\nprompt='Hello $ARGUMENTS'\n[skills.hidden]\ndescription='Hidden'\nprompt='Hidden'\nenabled=false").unwrap();
        let registry = Registry::new(custom, root.path(), &[]);
        assert_eq!(
            registry.load("HI", true).unwrap().expand(Some("there")),
            "Hello there"
        );
        assert!(registry.load("hidden", true).is_err());
        assert!(registry.load("commit", true).is_err());
        for text in [
            "version=2\n[skills]",
            "version=1\n[skills.bad]\ndescription='x'\nprompt=''",
            "version=1\n[skills.list]\ndescription='x'\nprompt='x'",
            "version=1\n[skills.a]\ndescription='a'\nprompt='a'\naliases=['b']\n[skills.b]\ndescription='b'\nprompt='b'",
        ] { assert!(bundled::parse_catalog(text).is_err(), "{text}"); }
    }
}
