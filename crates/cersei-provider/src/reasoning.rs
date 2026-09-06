//! Model-specific reasoning controls shared by the CLI and request encoder.

fn model_name(model: &str) -> String {
    model.rsplit(['/', ':']).next().unwrap_or(model).to_ascii_lowercase()
}

/// Explicit choices only; an omitted effort always keeps the server default.
pub fn levels(model: &str) -> &'static [&'static str] {
    let m = model_name(model);
    if m.starts_with("gpt-6") {
        &["low", "medium", "high", "xhigh", "max"]
    } else if m == "gpt-daybreak-blue-latest" || m == "daybreak-blue-latest" {
        &["low", "medium", "high", "xhigh", "max"]
    } else if m.starts_with("gpt-5.6") {
        &["none", "low", "medium", "high", "xhigh", "max"]
    } else if m.starts_with("gpt-5.2-pro") || m.starts_with("gpt-5.4-pro") {
        &["medium", "high", "xhigh"]
    } else if m == "gpt-5-pro" {
        &["high"]
    } else if m.starts_with("gpt-5.2") || m.starts_with("gpt-5.4") || m.starts_with("gpt-5.5") {
        &["none", "low", "medium", "high", "xhigh"]
    } else if m.starts_with("gpt-5.1") {
        &["none", "low", "medium", "high"]
    } else if m.starts_with("gpt-5") && !m.contains("codex") && !m.contains("chat") {
        &["minimal", "low", "medium", "high"]
    } else if m.starts_with("o3") || m.starts_with("o4-mini") {
        &["low", "medium", "high"]
    } else if m.starts_with("deepseek-v4") || m == "deepseek-chat" || m == "deepseek-reasoner" {
        &["none", "low", "high", "max"]
    } else if m.starts_with("kimi-k3") {
        &["low", "high", "max"]
    } else if m.starts_with("kimi-k2.5") || m.starts_with("kimi-k2.6") {
        &["none", "on"]
    } else if m.starts_with("gemini-3.1") || m.starts_with("gemini-3-flash") {
        &["low", "medium", "high"]
    } else if m.starts_with("gemini-3-pro") {
        &["low", "high"]
    } else if m.starts_with("gemini-2.5-flash") {
        &["none", "low", "medium", "high"]
    } else if m.starts_with("gemini-2.5-pro") {
        &["low", "medium", "high"]
    } else {
        &[]
    }
}

pub fn validate(model: &str, effort: &str) -> cersei_types::Result<()> {
    if levels(model).contains(&effort) {
        Ok(())
    } else {
        Err(cersei_types::CerseiError::Provider(format!(
            "Reasoning level '{effort}' is not supported for {model}. Available: default{}",
            levels(model).iter().map(|s| format!(", {s}")).collect::<String>()
        )))
    }
}

/// Modern OpenAI reasoning and function tools require the Responses endpoint.
pub(crate) fn uses_responses(model: &str) -> bool {
    let m = model_name(model);
    m.starts_with("gpt-5") || m.starts_with("gpt-6") || m == "gpt-daybreak-blue-latest" || m == "daybreak-blue-latest"
}

pub(crate) fn apply_chat(body: &mut serde_json::Value, model: &str, effort: &str) {
    let m = model_name(model);
    if m.starts_with("deepseek") || m.starts_with("kimi-k2.") {
        body["thinking"] = serde_json::json!({"type": if effort == "none" { "disabled" } else { "enabled" }});
        if effort != "none" && effort != "on" {
            body["reasoning_effort"] = effort.into();
        }
    } else {
        body["reasoning_effort"] = effort.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_specific_choices_reject_invalid_effort() {
        assert!(validate("gpt-5.6-luna", "max").is_ok());
        assert!(validate("gpt-5.6-luna", "ultra").is_err());
        assert!(validate("gpt-6-astra", "none").is_err());
        assert!(validate("kimi-k3", "none").is_err());
        assert!(validate("gemini-3.1-pro-preview", "max").is_err());
        assert!(validate("unknown-model", "high").is_err());
        assert_eq!(levels("openai/gpt-5.6-luna"), levels("gpt-5.6-luna"));
    }

    #[test]
    fn deepseek_can_toggle_thinking_without_switching_profiles() {
        let mut body = serde_json::json!({"model": "deepseek-chat"});
        apply_chat(&mut body, "deepseek-chat", "high");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
        let mut body = serde_json::json!({"model": "deepseek-reasoner"});
        apply_chat(&mut body, "deepseek-reasoner", "none");
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("reasoning_effort").is_none());
    }
}
