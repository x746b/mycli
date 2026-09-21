//! User-editable prompt content, independent of tool registration and provider settings.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const DEFAULTS: &str = include_str!("../../../system-prompts.toml");

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    #[serde(default)]
    pub description: String,
    pub prompt: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tier {
    guidelines: String,
    #[serde(default)]
    style: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Style {
    suppress_for_personas: Vec<String>,
    suppress_for_models: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    personas: BTreeMap<String, Persona>,
    #[serde(default)]
    tiers: BTreeMap<String, Tier>,
    #[serde(default)]
    style: Style,
}

pub struct Prompts {
    document: Document,
    source: Option<PathBuf>,
}

impl Prompts {
    fn parse(text: &str) -> Result<Self> {
        let mut document: Document = toml::from_str(text).context("invalid prompt TOML")?;
        if document.version != 1 {
            bail!(
                "unsupported prompt version {}; expected 1",
                document.version
            );
        }
        let mut personas = BTreeMap::new();
        for (name, persona) in document.personas {
            let key = name.trim().to_lowercase();
            if key.is_empty() || key.chars().any(char::is_whitespace) {
                bail!("persona names must be nonempty and contain no whitespace");
            }
            if key == "neutral" && !persona.prompt.is_empty() {
                bail!("neutral must have an empty prompt");
            }
            if personas.insert(key.clone(), persona).is_some() {
                bail!("duplicate persona name (case-insensitive): {key}");
            }
        }
        personas.entry("neutral".into()).or_insert_with(|| Persona {
            description: "No persona text or house style".into(),
            prompt: String::new(),
        });
        document.personas = personas;
        for name in document.tiers.keys() {
            if !["simple", "medium", "full"].contains(&name.as_str()) {
                bail!("unknown tool tier '{name}'");
            }
        }
        let defaults: Document = toml::from_str(DEFAULTS).expect("embedded prompts must parse");
        for (name, tier) in defaults.tiers {
            document.tiers.entry(name).or_insert(tier);
        }
        for pattern in &document.style.suppress_for_models {
            glob::Pattern::new(pattern).context("invalid model suppression glob")?;
        }
        Ok(Self {
            document,
            source: None,
        })
    }

    pub(crate) fn embedded() -> Self {
        Self::parse(DEFAULTS).expect("embedded prompts must be valid")
    }

    fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let mut prompts =
            Self::parse(&text).with_context(|| format!("could not load {}", path.display()))?;
        prompts.source = Some(path.to_owned());
        Ok(prompts)
    }

    fn load_paths(explicit: Option<PathBuf>, preferred: &Path, legacy: &Path) -> Result<Self> {
        if let Some(path) = explicit {
            return Self::from_path(&path);
        }
        if preferred.try_exists()? {
            return Self::from_path(preferred);
        }
        if legacy.try_exists()? {
            return Self::from_path(legacy);
        }
        Ok(Self::embedded())
    }

    pub fn load() -> Result<Self> {
        Self::load_paths(
            std::env::var_os("MYCLI_PROMPTS").map(PathBuf::from),
            &crate::config::global_config_dir().join("system-prompts.toml"),
            &crate::config::legacy_config_dir().join("system-prompts.toml"),
        )
    }

    pub fn startup() -> Self {
        Self::load().unwrap_or_else(|error| {
            eprintln!("Warning: {error:#}; using embedded system prompts");
            Self::embedded()
        })
    }

    pub fn path_info(&self) -> String {
        let source = self
            .source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "embedded defaults".into());
        let target = std::env::var_os("MYCLI_PROMPTS")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::config::global_config_dir().join("system-prompts.toml"));
        format!(
            "Active prompts: {source}\nPreferred file: {}",
            target.display()
        )
    }

    pub fn names(&self) -> Vec<String> {
        self.document.personas.keys().cloned().collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.document.personas.contains_key(&name.to_lowercase())
    }

    pub fn resolve_persona(&self, name: &str) -> &str {
        self.document
            .personas
            .get_key_value(&name.to_lowercase())
            .map(|(key, _)| key.as_str())
            .unwrap_or_else(|| {
                if self.contains("code") {
                    "code"
                } else {
                    "neutral"
                }
            })
    }

    pub fn description(&self, name: &str) -> &str {
        &self.document.personas[self.resolve_persona(name)].description
    }

    pub fn render(&self, persona: &str, tier: &str, model: &str) -> String {
        let name = self.resolve_persona(persona);
        let mut result = self.document.personas[name].prompt.clone();
        if !result.is_empty() {
            result.push('\n');
        }
        let tier = self
            .document
            .tiers
            .get(tier)
            .unwrap_or(&self.document.tiers["full"]);
        result.push_str(&tier.guidelines);
        let suppress = name == "neutral"
            || self
                .document
                .style
                .suppress_for_personas
                .iter()
                .any(|p| p.eq_ignore_ascii_case(name))
            || self
                .document
                .style
                .suppress_for_models
                .iter()
                .any(|pattern| {
                    glob::Pattern::new(&pattern.to_lowercase())
                        .expect("validated glob")
                        .matches(&model.to_lowercase())
                });
        if !suppress {
            result.push_str(&tier.style);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_prompt_text_preserved_for_all_tiers() {
        let prompts = Prompts::embedded();
        for (tier, expected) in [
            (
                "simple",
                include_str!("../tests/fixtures/legacy-code-simple.txt"),
            ),
            (
                "medium",
                include_str!("../tests/fixtures/legacy-code-medium.txt"),
            ),
            (
                "full",
                include_str!("../tests/fixtures/legacy-code-full.txt"),
            ),
        ] {
            assert_eq!(prompts.render("code", tier, "qwen"), expected);
        }
    }

    #[test]
    fn editable_catalog_replaces_personas_and_keeps_neutral() {
        let prompts =
            Prompts::parse("version = 1\n[personas.Reviewer]\nprompt = 'Review code.'").unwrap();
        assert_eq!(prompts.names(), ["neutral", "reviewer"]);
        assert_eq!(prompts.resolve_persona("Reviewer"), "reviewer");
        assert_eq!(prompts.resolve_persona("code"), "neutral");
        assert!(prompts
            .render("Reviewer", "simple", "qwen")
            .starts_with("Review code.\n"));
        assert_eq!(Prompts::embedded().resolve_persona("removed"), "code");
    }

    #[test]
    fn neutral_and_model_globs_suppress_only_style() {
        let prompts = Prompts::embedded();
        let neutral = prompts.render("Neutral", "medium", "qwen");
        assert_eq!(neutral, prompts.document.tiers["medium"].guidelines);
        let normal = prompts.render("code", "medium", "qwen");
        let suppressed = prompts.render("code", "medium", "DavidAU-COLD-FUSION-709");
        assert_eq!(normal, format!("{suppressed}- Be concise and direct.\n"));
        let custom = Prompts::parse("version=1\n[personas.custom]\nprompt='Hello'\n[style]\nsuppress_for_personas=['CUSTOM']").unwrap();
        assert!(!custom
            .render("custom", "simple", "qwen")
            .contains("Be concise"));
    }

    #[test]
    fn proposal_parses_and_does_not_lengthen_personas_or_tiers() {
        let proposed =
            Prompts::parse(include_str!("../../../docs/system-prompts.proposed.toml")).unwrap();
        let original = Prompts::embedded();
        for (name, persona) in &proposed.document.personas {
            assert!(
                persona.prompt.chars().count()
                    <= original.document.personas[name].prompt.chars().count(),
                "{name}"
            );
        }
        for (name, tier) in &proposed.document.tiers {
            let old = &original.document.tiers[name];
            assert!(
                tier.guidelines.chars().count() + tier.style.chars().count()
                    <= old.guidelines.chars().count() + old.style.chars().count(),
                "{name}"
            );
        }
    }

    #[test]
    fn invalid_content_is_rejected() {
        for text in [
            "version=2\n[personas]",
            "version=1\n[personas.neutral]\nprompt='not empty'",
            "version=1\n[personas.code]\nprompt=''\n[personas.Code]\nprompt=''",
            "version=1\n[personas]\n[style]\nsuppress_for_models=['[']",
            "version=1\n[personas]\n[tiers.typo]\nguidelines=''",
            "version=1\npersona={}",
        ] {
            assert!(Prompts::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn file_precedence_missing_files_and_reload_failure() {
        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("preferred.toml");
        let legacy = dir.path().join("legacy.toml");
        let explicit = dir.path().join("explicit.toml");
        assert!(Prompts::load_paths(None, &preferred, &legacy)
            .unwrap()
            .source
            .is_none());
        std::fs::write(&legacy, DEFAULTS).unwrap();
        assert_eq!(
            Prompts::load_paths(None, &preferred, &legacy)
                .unwrap()
                .source,
            Some(legacy.clone())
        );
        std::fs::write(&preferred, DEFAULTS).unwrap();
        assert_eq!(
            Prompts::load_paths(None, &preferred, &legacy)
                .unwrap()
                .source,
            Some(preferred.clone())
        );
        std::fs::write(&explicit, "version=1\n[personas.custom]\nprompt='custom'").unwrap();
        let active = Prompts::load_paths(Some(explicit.clone()), &preferred, &legacy).unwrap();
        assert!(active.contains("custom"));
        std::fs::write(&explicit, "broken TOML").unwrap();
        assert!(Prompts::load_paths(Some(explicit.clone()), &preferred, &legacy).is_err());
        assert!(active.contains("custom"));
        std::fs::remove_file(&explicit).unwrap();
        assert!(Prompts::load_paths(Some(explicit), &preferred, &legacy).is_err());
    }
}
