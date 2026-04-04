//! File edit tool: performs exact string replacements or line-range replacements.

use super::*;
use serde::Deserialize;

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str { "Edit" }
    fn description(&self) -> &str {
        "Edit files. Two modes:\n\
         1. String replacement: provide old_string and new_string to replace text.\n\
         2. Line range: provide start_line and end_line (1-based, inclusive) with new_string to replace those lines.\n\
         Line range mode is recommended when you know the line numbers from a previous Read."
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Write }
    fn category(&self) -> ToolCategory { ToolCategory::FileSystem }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file to edit" },
                "old_string": { "type": "string", "description": "The exact text to replace (for string replacement mode)" },
                "new_string": { "type": "string", "description": "The replacement text" },
                "start_line": { "type": "integer", "description": "Start line number, 1-based inclusive (for line range mode)" },
                "end_line": { "type": "integer", "description": "End line number, 1-based inclusive (for line range mode)" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences of old_string", "default": false }
            },
            "required": ["file_path", "new_string"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        #[derive(Deserialize)]
        struct Input {
            file_path: String,
            #[serde(default)]
            old_string: Option<String>,
            new_string: String,
            #[serde(default)]
            start_line: Option<usize>,
            #[serde(default)]
            end_line: Option<usize>,
            #[serde(default)]
            replace_all: bool,
        }

        let input: Input = match serde_json::from_value(input) {
            Ok(i) => i,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        let path = std::path::Path::new(&input.file_path);
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        // Decide mode: line-range or string replacement
        let new_content = if let (Some(start), Some(end)) = (input.start_line, input.end_line) {
            // Line-range mode
            if start == 0 || end == 0 {
                return ToolResult::error("start_line and end_line are 1-based, cannot be 0".to_string());
            }
            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            if start > total {
                return ToolResult::error(format!(
                    "start_line {} is beyond end of file ({} lines)", start, total
                ));
            }
            let end = end.min(total); // clamp to file length
            let start_idx = start - 1; // to 0-based

            let mut result = String::new();
            // Lines before the range
            for line in &lines[..start_idx] {
                result.push_str(line);
                result.push('\n');
            }
            // The replacement
            result.push_str(&input.new_string);
            if !input.new_string.ends_with('\n') {
                result.push('\n');
            }
            // Lines after the range
            for line in &lines[end..] {
                result.push_str(line);
                result.push('\n');
            }
            // Preserve original trailing newline behavior
            if !content.ends_with('\n') && result.ends_with('\n') {
                result.pop();
            }
            result
        } else if let Some(old_string) = &input.old_string {
            if old_string.is_empty() {
                return ToolResult::error("old_string cannot be empty".to_string());
            }
            // String replacement mode — try exact, then fuzzy
            if content.contains(old_string) {
                if input.replace_all {
                    content.replace(old_string, &input.new_string)
                } else {
                    let count = content.matches(old_string).count();
                    if count > 1 {
                        return ToolResult::error(format!(
                            "old_string is not unique ({} occurrences). Use replace_all or provide more context.",
                            count
                        ));
                    }
                    content.replacen(old_string, &input.new_string, 1)
                }
            } else if let Some(actual) = fuzzy_find_match(&content, old_string) {
                if input.replace_all {
                    content.replace(&actual, &input.new_string)
                } else {
                    let count = content.matches(actual.as_str()).count();
                    if count > 1 {
                        return ToolResult::error(format!(
                            "old_string is not unique ({} occurrences). Use replace_all or provide more context.",
                            count
                        ));
                    }
                    content.replacen(&actual, &input.new_string, 1)
                }
            } else {
                let old_preview: String = old_string.chars().take(80).collect();
                return ToolResult::error(format!(
                    "old_string not found in {}. Tip: use start_line/end_line instead — line numbers are shown by the Read tool. Your old_string started with: {:?}",
                    input.file_path, old_preview
                ));
            }
        } else {
            return ToolResult::error(
                "Provide either old_string or start_line+end_line to specify what to replace.".to_string()
            );
        };

        match tokio::fs::write(path, &new_content).await {
            Ok(()) => ToolResult::success(format!(
                "The file {} has been updated successfully.",
                input.file_path
            )),
            Err(e) => ToolResult::error(format!("Failed to write file: {}", e)),
        }
    }
}

/// Try to find a fuzzy match in the file content when exact match fails.
/// Handles common local-LLM mistakes: wrong indentation, trailing whitespace,
/// \r\n vs \n, and minor whitespace differences.
fn fuzzy_find_match(content: &str, old_string: &str) -> Option<String> {
    // Strategy 1: Trim trailing whitespace from each line and compare
    let normalize_lines = |s: &str| -> String {
        s.lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    };

    let norm_old = normalize_lines(old_string);
    let norm_content = normalize_lines(content);

    if let Some(start) = norm_content.find(&norm_old) {
        let norm_before = &norm_content[..start];
        let line_start = norm_before.matches('\n').count();
        let line_count = norm_old.matches('\n').count() + 1;

        let content_lines: Vec<&str> = content.lines().collect();
        if line_start + line_count <= content_lines.len() {
            let matched: String = content_lines[line_start..line_start + line_count].join("\n");
            if content.contains(&matched) {
                return Some(matched);
            }
        }
    }

    // Strategy 2: Strip all indentation and do line-by-line comparison
    let old_trimmed: String = old_string
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");

    if old_trimmed.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let old_lines: Vec<&str> = old_trimmed.lines().collect();

    if old_lines.is_empty() {
        return None;
    }

    for start_idx in 0..content_lines.len() {
        if start_idx + old_lines.len() > content_lines.len() {
            break;
        }
        let mut matches = true;
        for (j, old_line) in old_lines.iter().enumerate() {
            if content_lines[start_idx + j].trim() != *old_line {
                matches = false;
                break;
            }
        }
        if matches {
            let matched: String =
                content_lines[start_idx..start_idx + old_lines.len()].join("\n");
            return Some(matched);
        }
    }

    None
}
