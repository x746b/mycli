//! Web search through the oMLX server.
//!
//! oMLX exposes `POST /v1/web/search`, backed by whichever provider is
//! configured in its settings — DDGS, DuckDuckGo, Brave, or SearXNG — with the
//! result count and snippet/full-page choice set there too.
//!
//! Going through the server rather than calling a search API directly means
//! there is one place to configure search and one place the key lives, and it
//! works with no key at all when the server is set to DDGS. The cost is that
//! it only exists while the session is pointed at a local server; a cloud
//! provider has no such endpoint, so the tool is only registered for local
//! ones.

use async_trait::async_trait;
use cersei_tools::{PermissionLevel, Tool, ToolCategory, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

/// How long to wait on the server, which is itself waiting on a search
/// provider and possibly fetching pages.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

pub struct OmlxWebSearch {
    base_url: String,
    api_key: String,
}

impl OmlxWebSearch {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    results: Vec<SearchResult>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    snippet: String,
}

/// Search snippets arrive HTML-escaped, which reads badly in a prompt and
/// wastes tokens on entities the model has to decode itself.
fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        // Last: an escaped ampersand must not be undone before the entities
        // that contain one.
        .replace("&amp;", "&")
}

fn format_results(response: &SearchResponse) -> String {
    if response.results.is_empty() {
        return format!(
            "No results ({} returned nothing).",
            if response.provider.is_empty() { "search" } else { &response.provider }
        );
    }
    let mut out = format!("{} results via {}:\n", response.results.len(), response.provider);
    for (i, r) in response.results.iter().enumerate() {
        out.push_str(&format!("\n{}. {}\n   {}\n", i + 1, unescape(&r.title), r.url));
        let snippet = unescape(&r.snippet);
        if !snippet.trim().is_empty() {
            out.push_str(&format!("   {}\n", snippet.trim()));
        }
    }
    out
}

#[async_trait]
impl Tool for OmlxWebSearch {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web and return titles, URLs and snippets. Use it for \
         current information, documentation, and anything not already known. \
         Follow up with WebFetch to read a result in full."
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "What to search for" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q,
            _ => return ToolResult::error("WebSearch needs a non-empty 'query'."),
        };

        let client = match reqwest::Client::builder().timeout(TIMEOUT).build() {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("HTTP client error: {e}")),
        };

        let response = client
            .post(format!("{}/web/search", self.base_url))
            .header("authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Search request failed: {e}")),
        };

        let status = response.status();
        if !status.is_success() {
            // 404 means this server has no search endpoint at all, which is a
            // different problem from a search that failed.
            let hint = if status == reqwest::StatusCode::NOT_FOUND {
                " — this server has no /v1/web/search endpoint"
            } else {
                ""
            };
            return ToolResult::error(format!("Search returned HTTP {status}{hint}"));
        }

        match response.json::<SearchResponse>().await {
            Ok(body) if body.ok || !body.results.is_empty() => {
                ToolResult::success(format_results(&body))
            }
            Ok(body) => ToolResult::error(format!(
                "Search failed: {}",
                body.error.unwrap_or_else(|| "server reported not ok".into())
            )),
            Err(e) => ToolResult::error(format!("Could not read search response: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(results: Vec<SearchResult>) -> SearchResponse {
        SearchResponse { ok: true, provider: "brave".into(), results, error: None }
    }

    #[test]
    fn formats_results_for_a_prompt() {
        let out = format_results(&response(vec![SearchResult {
            title: "Qwen3 report".into(),
            url: "https://example.com/a".into(),
            snippet: "A summary.".into(),
        }]));
        assert!(out.contains("1 results via brave"), "{out}");
        assert!(out.contains("Qwen3 report"), "{out}");
        assert!(out.contains("https://example.com/a"), "{out}");
    }

    /// Snippets arrive HTML-escaped from the search provider.
    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape("&quot;hi&quot; &amp; bye"), "\"hi\" & bye");
        // `&amp;lt;` is a literal "&lt;", not a less-than sign: unescaping the
        // ampersand first would wrongly produce one.
        assert_eq!(unescape("a &lt;b&gt; c"), "a <b> c");
    }

    #[test]
    fn says_so_when_there_is_nothing() {
        let out = format_results(&response(vec![]));
        assert!(out.starts_with("No results"), "{out}");
    }
}
