//! Deterministic discovery shared by the CLI picker and model Skill tool.
use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn discover_all(project_root: Option<&Path>, extra_paths: &[PathBuf]) -> Vec<SkillMeta> {
    discover_with_bundled(project_root, extra_paths, &bundled::BUNDLED_SKILLS)
}

pub fn discover_with_bundled(
    project_root: Option<&Path>,
    extra_paths: &[PathBuf],
    internal: &[bundled::BundledSkill],
) -> Vec<SkillMeta> {
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    for skill in internal {
        let meta = bundled::load_bundled(skill, None).meta;
        seen.insert(meta.name.to_lowercase());
        seen.extend(meta.aliases.iter().map(|a| a.to_lowercase()));
        skills.push(meta);
    }
    for dir in build_search_dirs(project_root, extra_paths) {
        scan(&dir, &mut skills, &mut seen);
    }
    skills
}

pub fn build_search_dirs(project_root: Option<&Path>, extra_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let standard = [".claude/commands", ".claude/skills", ".agents/skills"];
    if let Some(root) = project_root {
        paths.push(root.join(".config/mycli/skills"));
        paths.extend(standard.iter().map(|p| root.join(p)));
    }
    if let Some(home) = dirs::home_dir() {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        paths.push(config.join("mycli/skills"));
        paths.extend(standard.iter().map(|p| home.join(p)));
    }
    paths.extend_from_slice(extra_paths);
    let mut seen = HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    paths
}

fn scan(dir: &Path, skills: &mut Vec<SkillMeta>, seen: &mut HashSet<String>) {
    // Accept a configured individual SKILL.md, skill folder, or collection root.
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .follow_links(true)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    files.sort();
    for path in files {
        let is_skill = path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md");
        // Supporting references are not standalone commands.
        if !is_skill
            && path
                .parent()
                .into_iter()
                .flat_map(|p| p.ancestors())
                .take_while(|p| p.starts_with(dir))
                .any(|p| p.join("SKILL.md").is_file())
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (fm, body) = match try_parse_frontmatter(&content) {
            Ok(parsed) => parsed,
            Err(error) => {
                eprintln!("Warning: skipping skill {}: {error}", path.display());
                continue;
            }
        };
        let fallback = if is_skill {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            path.strip_prefix(dir)
                .unwrap_or(&path)
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join(":")
        };
        let name = fm.get("name").cloned().unwrap_or(fallback);
        if name.is_empty() || seen.contains(&name.to_lowercase()) || body.trim().is_empty() {
            continue;
        }
        let absolute = std::fs::canonicalize(&path).unwrap_or(path);
        let list = |key: &str| {
            fm.get(key).map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>()
            })
        };
        let aliases: Vec<String> = list("aliases")
            .unwrap_or_default()
            .into_iter()
            .filter(|a: &String| !seen.contains(&a.to_lowercase()))
            .collect();
        seen.insert(name.to_lowercase());
        seen.extend(aliases.iter().map(|a| a.to_lowercase()));
        skills.push(SkillMeta {
            name,
            description: fm
                .get("description")
                .cloned()
                .unwrap_or_else(|| extract_description(&body)),
            path: Some(absolute.display().to_string()),
            bundled: false,
            aliases,
            allowed_tools: list("allowed-tools"),
            argument_hint: fm.get("argument-hint").cloned(),
            format: if is_skill {
                SkillFormat::OpenCode
            } else {
                SkillFormat::ClaudeCode
            },
            user_invocable: fm
                .get("user-invocable")
                .map(|s| s != "false")
                .unwrap_or(true),
            model_invocable: fm
                .get("disable-model-invocation")
                .map(|s| s != "true")
                .unwrap_or(true),
        });
    }
}

/// Load the exact discovered path (frontmatter names need not match folder names).
pub fn load_discovered(
    meta: &SkillMeta,
    internal: &[bundled::BundledSkill],
) -> Result<LoadedSkill, String> {
    if meta.bundled {
        return internal
            .iter()
            .find(|s| s.name == meta.name)
            .map(|s| bundled::load_bundled(s, None))
            .ok_or_else(|| format!("Skill '{}' no longer exists", meta.name));
    }
    let path = meta.path.as_ref().ok_or("Missing skill path")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let (_, content) = try_parse_frontmatter(&text)?;
    Ok(LoadedSkill {
        meta: meta.clone(),
        content,
    })
}

pub fn load_skill(
    name: &str,
    project_root: Option<&Path>,
    extra_paths: &[PathBuf],
) -> Option<LoadedSkill> {
    let all = discover_all(project_root, extra_paths);
    let meta = all.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name) || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })?;
    load_discovered(meta, &bundled::BUNDLED_SKILLS).ok()
}

/// Format skill list for display (compatible with Claude Code's skill list output).
pub fn format_skill_list(skills: &[SkillMeta]) -> String {
    if skills.is_empty() {
        return "No skills available.".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Available skills:".to_string());

    for skill in skills {
        let tag = if skill.bundled {
            " [bundled]".to_string()
        } else {
            format!(" [{}]", skill.path.as_deref().unwrap_or("external"))
        };
        let hint = skill
            .argument_hint
            .as_deref()
            .map(|h| format!(" {}", h))
            .unwrap_or_default();
        lines.push(format!(
            "  {}{} — {}{}",
            skill.name, hint, skill.description, tag
        ));
        if !skill.aliases.is_empty() {
            lines.push(format!("    aliases: {}", skill.aliases.join(", ")));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_bundled() {
        let skills = discover_all(None, &[]);
        assert!(!skills.is_empty());
        assert!(skills.iter().any(|s| s.name == "simplify"));
        assert!(skills.iter().any(|s| s.name == "debug"));
        assert!(skills.iter().any(|s| s.name == "commit"));
    }

    #[test]
    fn test_discover_claude_code_format() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd_dir = tmp.path().join(".claude/commands");
        fs::create_dir_all(&cmd_dir).unwrap();
        fs::write(
            cmd_dir.join("my-skill.md"),
            "---\ndescription: My custom skill\n---\n\nDo $ARGUMENTS please.",
        )
        .unwrap();

        let skills = discover_all(Some(tmp.path()), &[]);
        let custom = skills.iter().find(|s| s.name == "my-skill");
        assert!(custom.is_some(), "Should discover Claude Code format skill");
        assert_eq!(custom.unwrap().description, "My custom skill");
        assert_eq!(custom.unwrap().format, SkillFormat::ClaudeCode);
    }

    #[test]
    fn test_discover_opencode_format() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join(".claude/skills/my-oc-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-oc-skill\ndescription: OpenCode style skill\n---\n\n# Skill content",
        )
        .unwrap();

        let skills = discover_all(Some(tmp.path()), &[]);
        let custom = skills.iter().find(|s| s.name == "my-oc-skill");
        assert!(custom.is_some(), "Should discover OpenCode format skill");
        assert_eq!(custom.unwrap().format, SkillFormat::OpenCode);
    }

    #[test]
    fn test_bundled_takes_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd_dir = tmp.path().join(".claude/commands");
        fs::create_dir_all(&cmd_dir).unwrap();
        // Create a disk skill with same name as bundled
        fs::write(cmd_dir.join("simplify.md"), "# Overridden simplify").unwrap();

        let skills = discover_all(Some(tmp.path()), &[]);
        let simplify = skills.iter().find(|s| s.name == "simplify").unwrap();
        assert!(simplify.bundled, "Bundled should take precedence over disk");
    }

    #[test]
    fn test_load_bundled_skill() {
        let loaded = load_skill("debug", None, &[]);
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert!(loaded.meta.bundled);
        assert!(loaded.content.contains("$ARGUMENTS"));
    }

    #[test]
    fn test_load_disk_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd_dir = tmp.path().join(".claude/commands");
        fs::create_dir_all(&cmd_dir).unwrap();
        fs::write(
            cmd_dir.join("deploy.md"),
            "---\ndescription: Deploy to prod\n---\n\nRun deploy for $ARGUMENTS",
        )
        .unwrap();

        let loaded = load_skill("deploy", Some(tmp.path()), &[]);
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert!(!loaded.meta.bundled);
        let expanded = loaded.expand(Some("staging"));
        assert!(expanded.contains("Run deploy for staging"));
    }

    #[test]
    fn test_load_from_extra_path() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("custom-skill.md"), "Do custom things").unwrap();

        let loaded = load_skill("custom-skill", None, &[tmp.path().to_path_buf()]);
        assert!(loaded.is_some());
    }

    #[test]
    fn test_real_claude_commands() {
        // Check if the user's actual ~/.claude/commands/ has skills
        let home_cmds = dirs::home_dir().map(|h| h.join(".claude/commands"));
        if let Some(dir) = home_cmds {
            if dir.exists() {
                let skills = discover_all(None, &[]);
                let disk_skills: Vec<_> = skills.iter().filter(|s| !s.bundled).collect();
                println!(
                    "Found {} disk skills from ~/.claude/commands/",
                    disk_skills.len()
                );
                for s in &disk_skills {
                    println!("  {} — {} ({:?})", s.name, s.description, s.format);
                }
            }
        }
    }

    #[test]
    fn test_format_skill_list() {
        let skills = discover_all(None, &[]);
        let formatted = format_skill_list(&skills);
        assert!(formatted.contains("Available skills:"));
        assert!(formatted.contains("simplify"));
        assert!(formatted.contains("[bundled]"));
    }
}
