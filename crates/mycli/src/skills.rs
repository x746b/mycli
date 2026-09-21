//! Skill configuration and slash-command support, independent of model/tool tier.
use crate::config::Config;
use anyhow::{Context, Result};
use cersei_tools::skills::{
    bundled, discovery,
    registry::{invocation, Registry},
};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};

pub struct Skills {
    pub registry: Arc<RwLock<Registry>>,
    pub source: String,
    pub paths: Vec<PathBuf>,
}

impl Skills {
    fn paths(config: &Config) -> Vec<PathBuf> {
        config
            .skill_paths
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|value| {
                if let Some(relative) = value.strip_prefix("~/") {
                    return dirs::home_dir().unwrap_or_default().join(relative);
                }
                let path = PathBuf::from(value);
                if path.is_absolute() {
                    path
                } else {
                    config.working_dir.join(path)
                }
            })
            .collect()
    }

    pub fn preferred_path() -> PathBuf {
        std::env::var_os("MYCLI_SKILLS")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::config::global_config_dir().join("skills-internal.toml"))
    }

    fn internal() -> Result<(Vec<bundled::BundledSkill>, String)> {
        let preferred = Self::preferred_path();
        let legacy = crate::config::legacy_config_dir().join("skills-internal.toml");
        let path = if preferred.try_exists()? || std::env::var_os("MYCLI_SKILLS").is_some() {
            Some(preferred)
        } else if legacy.try_exists()? {
            Some(legacy)
        } else {
            None
        };
        if let Some(path) = path {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Cannot read {}", path.display()))?;
            let internal = bundled::parse_catalog(&text)
                .map_err(anyhow::Error::msg)
                .with_context(|| format!("Invalid skills catalog {}", path.display()))?;
            Ok((internal, path.display().to_string()))
        } else {
            Ok((bundled::BUNDLED_SKILLS.clone(), "embedded defaults".into()))
        }
    }

    pub fn load(config: &Config) -> Result<Self> {
        let (internal, source) = Self::internal()?;
        let extra = Self::paths(config);
        Ok(Self {
            registry: Arc::new(RwLock::new(Registry::new(
                internal,
                &config.working_dir,
                &extra,
            ))),
            source,
            paths: discovery::build_search_dirs(Some(&config.working_dir), &extra),
        })
    }

    pub fn startup(config: &Config) -> Self {
        Self::load(config).unwrap_or_else(|error| {
            eprintln!("Warning: {error:#}; using embedded internal skills");
            let extra = Self::paths(config);
            Self {
                registry: Arc::new(RwLock::new(Registry::new(
                    bundled::BUNDLED_SKILLS.clone(),
                    &config.working_dir,
                    &extra,
                ))),
                source: "embedded defaults".into(),
                paths: discovery::build_search_dirs(Some(&config.working_dir), &extra),
            }
        })
    }

    pub fn reload(&mut self, config: &Config) -> Result<()> {
        let next = Self::load(config)?;
        *self.registry.write() = Arc::try_unwrap(next.registry)
            .ok()
            .expect("new unshared registry")
            .into_inner();
        self.source = next.source;
        self.paths = next.paths;
        Ok(())
    }

    pub fn expand(&self, name: &str, args: Option<&str>) -> Result<String> {
        let skill = self
            .registry
            .read()
            .load(name, true)
            .map_err(anyhow::Error::msg)?;
        Ok(invocation(&skill, args))
    }

    pub fn path_info(&self) -> String {
        format!("Internal catalog: {}\nPreferred file: {}\nSearch paths (first match wins after internal skills):\n{}",
            self.source, Self::preferred_path().display(),
            self.paths.iter().map(|p| format!("  {}{}", p.display(), if p.exists() { "" } else { " (missing)" })).collect::<Vec<_>>().join("\n"))
    }
}
