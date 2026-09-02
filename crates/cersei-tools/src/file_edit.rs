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
            // String replacement mode. The replacer ladder tries exact first,
            // then progressively more tolerant strategies — weaker models drift
            // on indentation, and a byte-exact requirement loses the edit. Every
            // strategy only ever returns text that really exists in the file, so
            // a fuzzy match relaxes where the text is found, never what is
            // written. See `tool_primitives::replace`.
            match crate::tool_primitives::replace::replace(
                &content,
                old_string,
                &input.new_string,
                input.replace_all,
            ) {
                Ok(updated) => updated,
                Err(crate::tool_primitives::replace::ReplaceError::Ambiguous { count }) => {
                    return ToolResult::error(format!(
                        "old_string is not unique ({} occurrences). Use replace_all or provide more context.",
                        count
                    ));
                }
                Err(crate::tool_primitives::replace::ReplaceError::NoChange) => {
                    return ToolResult::error(
                        "old_string and new_string are identical — nothing to change.".to_string(),
                    );
                }
                Err(crate::tool_primitives::replace::ReplaceError::EmptyOldString) => {
                    return ToolResult::error("old_string cannot be empty".to_string());
                }
                Err(crate::tool_primitives::replace::ReplaceError::NotFound) => {
                    let old_preview: String = old_string.chars().take(80).collect();
                    return ToolResult::error(format!(
                        "old_string not found in {}. Re-read the file — it may have changed. If the text appears more than once, add surrounding lines to disambiguate, or use start_line/end_line (line numbers are shown by Read). Your old_string started with: {:?}",
                        input.file_path, old_preview
                    ));
                }
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


#[cfg(test)]
mod tolerance_tests {
    use crate::tool_primitives::replace::replace;

    /// The failure this port exists for: a model quotes code back with the
    /// wrong indentation, which an exact match rejects outright.
    #[test]
    fn an_edit_survives_indentation_drift() {
        let file = "class S:\n    def go(self):\n        x = compute()\n        return x\n";
        // Model wrote four spaces where the file has eight.
        let drifted = "    x = compute()\n    return x";
        assert!(
            !file.contains(drifted),
            "test is vacuous unless an exact match genuinely fails"
        );
        let out = replace(file, drifted, "    x = compute() * 2\n    return x", false)
            .expect("ladder should locate the block despite the drift");
        assert!(out.contains("compute() * 2"), "{out}");
        // The rest of the file is untouched.
        assert!(out.starts_with("class S:\n    def go(self):\n"), "{out}");
    }
}
