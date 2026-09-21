//! Skills system: discover, load, and execute skill prompt templates.
//!
//! Compatible with:
//! - **Claude Code**: `.claude/commands/*.md` with `$ARGUMENTS` expansion
//! - **OpenCode**: `.claude/skills/**/SKILL.md` with YAML frontmatter
//! - **Custom directories**: any path passed to the scanner

pub mod bundled;
pub mod discovery;
pub mod registry;

use serde::{Deserialize, Serialize};

/// Metadata about a discovered skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    /// Skill name (filename without .md, or frontmatter `name:` field).
    pub name: String,
    /// One-line description (from frontmatter or first content line).
    pub description: String,
    /// Absolute path to the .md file (None for bundled skills).
    pub path: Option<String>,
    /// Whether this is a built-in bundled skill.
    pub bundled: bool,
    /// Alternative names for this skill.
    pub aliases: Vec<String>,
    /// Declared tool metadata; loading a skill does not change runtime permissions.
    pub allowed_tools: Option<Vec<String>>,
    /// Usage hint for arguments.
    pub argument_hint: Option<String>,
    /// Skill format detected.
    pub format: SkillFormat,
    #[serde(default = "default_invocable")]
    pub user_invocable: bool,
    #[serde(default = "default_invocable")]
    pub model_invocable: bool,
}

fn default_invocable() -> bool { true }

/// Which format the skill file uses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SkillFormat {
    /// Claude Code format: .claude/commands/<name>.md
    ClaudeCode,
    /// OpenCode format: .claude/skills/<name>/SKILL.md with required frontmatter
    OpenCode,
    /// Bundled (compiled into binary)
    Bundled,
}

/// A loaded skill ready for expansion.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub meta: SkillMeta,
    /// Raw content (frontmatter stripped).
    pub content: String,
}

impl LoadedSkill {
    /// Expand the skill template with given arguments.
    pub fn expand(&self, args: Option<&str>) -> String {
        let args = args.unwrap_or("");
        let words: Vec<&str> = args.split_whitespace().collect();
        let directory = self.meta.path.as_ref().and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.display().to_string()).unwrap_or_default();
        let pattern = regex::Regex::new(r"\$ARGUMENTS_SUFFIX|\$ARGUMENTS\[(\d+)\]|\$ARGUMENTS|\$(\d+)|\$\{CLAUDE_SKILL_DIR\}").unwrap();
        pattern.replace_all(&self.content, |caps: &regex::Captures<'_>| {
            if let Some(index) = caps.get(1).or_else(|| caps.get(2)) {
                return index.as_str().parse::<usize>().ok().and_then(|i| words.get(i)).unwrap_or(&"").to_string();
            }
            match &caps[0] {
                "$ARGUMENTS_SUFFIX" if !args.is_empty() => format!(": {args}"),
                "$ARGUMENTS_SUFFIX" => String::new(),
                "${CLAUDE_SKILL_DIR}" => directory.clone(),
                _ => args.to_string(),
            }
        }).into_owned()
    }
}

/// Strip YAML frontmatter from content (Claude Code compatible).
/// Handles `---\n...\n---\n` format.
pub fn strip_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        let after_open = &content[3..];
        if let Some(close_pos) = after_open.find("\n---") {
            let rest = &after_open[close_pos + 4..];
            return rest.trim_start_matches('\n').to_string();
        }
    }
    content.to_string()
}

/// Parse YAML frontmatter into key-value pairs.
/// Returns (frontmatter_map, content_after_frontmatter).
pub fn parse_frontmatter(content: &str) -> (std::collections::HashMap<String, String>, String) {
    try_parse_frontmatter(content).unwrap_or_else(|_| (Default::default(), strip_frontmatter(content)))
}

/// Parse scalar and list YAML metadata, including quoted and multiline values.
pub fn try_parse_frontmatter(content: &str) -> Result<(std::collections::HashMap<String, String>, String), String> {
    let normalized = content.replace("\r\n", "\n");
    let Some(after) = normalized.strip_prefix("---\n") else {
        return Ok((Default::default(), content.to_string()));
    };
    let mut offset = 0;
    for line in after.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let value: serde_yaml::Value = serde_yaml::from_str(&after[..offset]).map_err(|e| e.to_string())?;
            let mut map = std::collections::HashMap::new();
            if let serde_yaml::Value::Mapping(fields) = value {
                for (key, value) in fields {
                    let Some(key) = key.as_str() else { continue };
                    let text = match value {
                        serde_yaml::Value::String(value) => value.trim().to_string(),
                        serde_yaml::Value::Bool(value) => value.to_string(),
                        serde_yaml::Value::Sequence(values) => values.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "),
                        _ => continue,
                    };
                    map.insert(key.to_owned(), text);
                }
            } else if !value.is_null() { return Err("Skill frontmatter must be a YAML mapping".into()); }
            return Ok((map, after[offset+line.len()..].trim_start_matches('\n').to_string()));
        }
        offset += line.len();
    }
    Err("Unclosed skill frontmatter".into())
}

/// Extract description from content: first non-empty line (max 80 chars).
/// Headings are stripped of their `#` prefix.
pub fn extract_description(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        // Strip heading markers
        let trimmed = trimmed.trim_start_matches('#').trim();
        // By characters: a byte-indexed cut panics on any description whose
        // 77th byte falls inside a multi-byte character.
        let desc = if trimmed.chars().count() > 80 {
            format!("{}...", trimmed.chars().take(77).collect::<String>())
        } else {
            trimmed.to_string()
        };
        return desc;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_with_yaml() {
        let content = "---\nname: test\ndescription: A test skill\n---\n\n# Body\n\nContent here.";
        let stripped = strip_frontmatter(content);
        assert!(stripped.starts_with("# Body"));
        assert!(!stripped.contains("name: test"));
    }

    #[test]
    fn test_strip_frontmatter_without_yaml() {
        let content = "# Just a heading\n\nSome content.";
        let stripped = strip_frontmatter(content);
        assert_eq!(stripped, content);
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\nname: my-skill\ndescription: Does things\nallowed-tools: Read, Write\n---\n\nBody";
        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.get("name").unwrap(), "my-skill");
        assert_eq!(fm.get("description").unwrap(), "Does things");
        assert!(body.starts_with("Body"));
    }

    #[test]
    fn test_expand_with_arguments() {
        let skill = LoadedSkill {
            meta: SkillMeta {
                name: "test".into(),
                description: "test".into(),
                path: None,
                bundled: true,
                aliases: vec![],
                allowed_tools: None,
                argument_hint: None,
                format: SkillFormat::Bundled,
                user_invocable: true, model_invocable: true,
            },
            content: "Do $ARGUMENTS in the codebase$ARGUMENTS_SUFFIX".into(),
        };

        let expanded = skill.expand(Some("fix tests"));
        assert_eq!(expanded, "Do fix tests in the codebase: fix tests");

        let expanded_empty = skill.expand(None);
        assert_eq!(expanded_empty, "Do  in the codebase");
    }

    #[test]
    fn test_extract_description() {
        assert_eq!(
            extract_description("# Heading\n\nFirst real line here."),
            "Heading"
        );
        // extract_description works on raw content — frontmatter lines are skipped by the --- check
        assert_eq!(
            extract_description(&strip_frontmatter("---\nfoo\n---\nContent after FM")),
            "Content after FM"
        );
        assert_eq!(extract_description(""), "");
    }
}
