//! Streamable HTTP MCP. Each POST accepts JSON or incremental SSE responses.
use crate::jsonrpc;
use cersei_types::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
pub struct HttpTransport {
    client: reqwest::Client,
    url: reqwest::Url,
    session: Option<HeaderValue>,
    protocol: Option<String>,
    initialize: Option<Value>,
    next_id: u64,
}
fn error(message: &str) -> CerseiError { CerseiError::Mcp(message.into()) }
impl HttpTransport {
    pub fn new(url: &str, headers: &HashMap<String, String>) -> Result<Self> {
        let url = reqwest::Url::parse(url).map_err(|_| error("Invalid MCP URL"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
            || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(error("MCP URL must be http(s), without userinfo or fragment"));
        }
        let mut defaults = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| error("Invalid MCP header name"))?;
            if matches!(name.as_str(), "host" | "content-length" | "transfer-encoding" | "content-type" | "accept" | "mcp-session-id" | "mcp-protocol-version") {
                return Err(error("MCP configuration overrides a transport-owned header"));
            }
            let mut value = HeaderValue::from_str(value).map_err(|_| error("Invalid MCP header value"))?;
            value.set_sensitive(true);
            defaults.insert(name, value);
        }
        let client = reqwest::Client::builder().default_headers(defaults)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(30))
            .build().map_err(|_| error("Could not build MCP HTTP client"))?;
        Ok(Self { client, url, session: None, protocol: None, initialize: None, next_id: 1 })
    }
    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut request = self.client.post(self.url.clone())
            .header("Accept", "application/json, text/event-stream").json(body);
        if let Some(session) = &self.session { request = request.header("Mcp-Session-Id", session); }
        if let Some(protocol) = &self.protocol { request = request.header("MCP-Protocol-Version", protocol); }
        request
    }
    pub async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id; self.next_id += 1;
        let body = serde_json::to_value(jsonrpc::Request::new(id, method, params.clone()))?;
        let response = self.round_trip(&body, Some(id)).await;
        if matches!(response, Err(CerseiError::ProviderStatus { status: 404, .. })) && self.session.is_some() {
            self.session = None;
            self.protocol = None;
            // Renew the session, but never replay a possibly side-effecting tool call.
            if let Some(params) = self.initialize.clone() {
                let init_id = self.next_id; self.next_id += 1;
                let init = json!({"jsonrpc":"2.0","id":init_id,"method":"initialize","params":params});
                let initialized = self.round_trip(&init, Some(init_id)).await?;
                self.set_protocol(&initialized)?;
                self.notify("notifications/initialized", None).await?;
                return Err(error("MCP session expired and was renewed; original request was not retried"));
            }
        }
        let response = response?;
        if method == "initialize" {
            self.set_protocol(&response)?;
            self.initialize = params;
        }
        Ok(response)
    }
    fn set_protocol(&mut self, response: &Value) -> Result<()> {
        let version = response.get("protocolVersion").and_then(Value::as_str)
            .ok_or_else(|| error("MCP initialize response has no protocolVersion"))?;
        if !matches!(version, "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25") {
            return Err(error("MCP server selected an unsupported protocol version"));
        }
        self.protocol = Some(version.into());
        Ok(())
    }
    pub async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let body = serde_json::to_value(jsonrpc::Request::notification(method, params))?;
        self.round_trip(&body, None).await.map(|_| ())
    }
    async fn round_trip(&mut self, body: &Value, id: Option<u64>) -> Result<Value> {
        tokio::time::timeout(Duration::from_secs(30), self.exchange(body, id)).await
            .map_err(|_| error("MCP HTTP exchange timed out (30s)"))?
    }
    async fn exchange(&mut self, body: &Value, id: Option<u64>) -> Result<Value> {
        let mut response = self.post(body).send().await
            .map_err(|_| error("MCP HTTP request failed or timed out"))?;
        if !response.status().is_success() {
            return Err(CerseiError::ProviderStatus { status: response.status().as_u16(),
                message: "MCP HTTP request rejected (URL, headers and body omitted)".into() });
        }
        if body.get("method").and_then(Value::as_str) == Some("initialize") {
            if let Some(session) = response.headers().get("mcp-session-id") {
                if session.as_bytes().is_empty() || !session.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b)) {
                    return Err(error("Invalid MCP session ID"));
                }
                let mut session = session.clone(); session.set_sensitive(true);
                self.session = Some(session);
            }
        }
        let Some(id) = id else {
            if response.status() != reqwest::StatusCode::ACCEPTED {
                return Err(error("MCP notification expected HTTP 202"));
            }
            return Ok(Value::Null);
        };
        let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok())
            .unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if content_type != "application/json" && content_type != "text/event-stream" {
            return Err(error("MCP response must be application/json or text/event-stream"));
        }
        if response.content_length().is_some_and(|n| n > MAX_RESPONSE as u64) {
            return Err(error("MCP HTTP response exceeded 16 MiB"));
        }
        let sse = content_type == "text/event-stream";
        let mut buffer = Vec::new();
        let mut data = String::new();
        let mut total = 0usize;
        while let Some(chunk) = response.chunk().await.map_err(|_| error("MCP response interrupted or timed out"))? {
            total = total.saturating_add(chunk.len());
            if total > MAX_RESPONSE { return Err(error("MCP HTTP response exceeded 16 MiB")); }
            buffer.extend_from_slice(&chunk);
            if sse {
                let mut consumed = 0;
                while let Some(rel) = buffer[consumed..].iter().position(|b| *b == b'\n') {
                    let end = consumed + rel;
                    let line = std::str::from_utf8(&buffer[consumed..end]).map_err(|_| error("Invalid UTF-8 in MCP SSE"))?.trim_end_matches('\r');
                    consumed = end + 1;
                    if line.is_empty() {
                        if !data.is_empty() {
                            let message: Value = serde_json::from_str(data.trim_end()).map_err(|_| error("Invalid JSON in MCP SSE"))?;
                            data.clear();
                            if let Some(result) = self.message(message, id).await? { return Ok(result); }
                        }
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push_str(value.strip_prefix(' ').unwrap_or(value));
                        data.push('\n');
                    }
                }
                buffer.drain(..consumed);
            }
        }
        if sse { return Err(error("MCP SSE stream ended without a matching response")); }
        let message: Value = serde_json::from_slice(&buffer).map_err(|_| error("Invalid MCP JSON response"))?;
        self.message(message, id).await?.ok_or_else(|| error("MCP response ID does not match request"))
    }
    async fn message(&self, message: Value, id: u64) -> Result<Option<Value>> {
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(error("Invalid MCP JSON-RPC version"));
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(server_id) = message.get("id") {
                let reply = if method == "ping" { json!({"jsonrpc":"2.0","id":server_id,"result":{}}) }
                    else { json!({"jsonrpc":"2.0","id":server_id,"error":{"code":-32601,"message":"Client method not supported"}}) };
                let response = self.post(&reply).send().await.map_err(|_| error("MCP server-request reply failed"))?;
                if response.status() != reqwest::StatusCode::ACCEPTED { return Err(error("MCP server-request reply rejected")); }
            }
            return Ok(None);
        }
        if message.get("id").and_then(Value::as_u64) != Some(id) { return Ok(None); }
        if let Some(err) = message.get("error") {
            // Do not echo an untrusted server error containing credentials or terminal escapes.
            return Err(CerseiError::Mcp(format!("MCP JSON-RPC error (code {})", err.get("code").unwrap_or(&Value::Null))));
        }
        message.get("result").cloned().map(Some).ok_or_else(|| error("MCP response missing result"))
    }
}
impl Drop for HttpTransport {
    fn drop(&mut self) {
        if let (Some(session), Ok(runtime)) = (self.session.take(), tokio::runtime::Handle::try_current()) {
            let mut request = self.client.delete(self.url.clone()).header("Mcp-Session-Id", session);
            if let Some(version) = &self.protocol { request = request.header("MCP-Protocol-Version", version); }
            runtime.spawn(async move { let _ = request.send().await; });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Reply { status: u16, kind: &'static str, body: String, session: bool, hold: bool }
    fn reply(body: Value) -> Reply { Reply { status: 200, kind: "application/json", body: body.to_string(), session: false, hold: false } }
    async fn server(replies: Vec<Reply>) -> (String, tokio::task::JoinHandle<Vec<(String, Value)>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).await.unwrap(); assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..pos]).to_ascii_lowercase();
                        let length = headers.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().parse::<usize>().unwrap();
                        break (pos + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let mut buf = [0; 4096]; let n = socket.read(&mut buf).await.unwrap(); assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                requests.push((String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase(), serde_json::from_slice(&bytes[header_end..header_end+length]).unwrap()));
                let length = if reply.hold { String::new() } else { format!("Content-Length: {}\r\n", reply.body.len()) };
                let headers = format!("HTTP/1.1 {} Mock\r\nContent-Type: {}\r\n{}{}Connection: close\r\n\r\n", reply.status, reply.kind, length,
                    if reply.session { "Mcp-Session-Id: test-session\r\n" } else { "" });
                socket.write_all(headers.as_bytes()).await.unwrap();
                // Deliberately fragment lines and multibyte UTF-8 across TCP writes.
                for chunk in reply.body.as_bytes().chunks(7) { socket.write_all(chunk).await.unwrap(); }
                if reply.hold { tokio::time::sleep(Duration::from_millis(500)).await; }
            }
            requests
        });
        (url, task)
    }
    #[tokio::test]
    async fn discovers_and_calls_over_http_with_session_and_auth() {
        let mut init = reply(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18"}})); init.session = true;
        let mut notification = reply(Value::Null); notification.status = 202; notification.body.clear();
        let (url, server) = server(vec![init, notification,
            reply(json!({"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}})),
            reply(json!({"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"not supported"}})),
            Reply { status: 200, kind: "text/event-stream", session: false, hold: true,
                body: ": heartbeat\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":4,\r\ndata: \"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello 🦀\"}]}}\r\n\r\n".into() },
        ]).await;
        let mut config = crate::McpServerConfig::http("mock", url);
        config.headers.insert("Authorization".into(), "Bearer test-token".into());
        let mut client = crate::McpClient::connect(config).await.unwrap();
        assert_eq!(client.tools[0].name, "echo");
        let text = tokio::time::timeout(Duration::from_millis(300), client.call_tool("echo", None)).await.unwrap().unwrap();
        assert_eq!(text, "hello 🦀");
        let requests = server.await.unwrap();
        for (i, (headers, _)) in requests.iter().enumerate() {
            assert!(headers.contains("accept: application/json, text/event-stream"));
            assert!(headers.contains("authorization: bearer test-token"));
            assert_eq!(headers.contains("mcp-session-id: test-session"), i > 0);
            assert_eq!(headers.contains("mcp-protocol-version: 2025-06-18"), i > 0);
        }
        assert!(requests[1].1.get("id").is_none());
    }
    #[tokio::test]
    async fn rejects_bad_ids_json_rpc_errors_and_auth_failures() {
        for (response, expected) in [
            (reply(json!({"jsonrpc":"2.0","id":99,"result":{}})), "ID"),
            (reply(json!({"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"secret-value"}})), "code -1"),
            (Reply { status: 401, kind: "text/plain", body: "secret-value".into(), session: false, hold: false }, "401"),
        ] {
            let (url, task) = server(vec![response]).await;
            let mut transport = HttpTransport::new(&url, &HashMap::new()).unwrap();
            let error = transport.request("tools/list", None).await.unwrap_err().to_string();
            assert!(error.contains(expected), "{error}"); assert!(!error.contains("secret-value"));
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn renews_expired_session_without_replaying_tool() {
        let mut init = reply(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18"}})); init.session = true;
        let mut renewed = reply(json!({"jsonrpc":"2.0","id":3,"result":{"protocolVersion":"2025-06-18"}})); renewed.session = true;
        let mut notify = reply(Value::Null); notify.status = 202; notify.body.clear();
        let (url, task) = server(vec![init, Reply {status:404, kind:"text/plain", body:String::new(), session:false, hold:false}, renewed, notify]).await;
        let mut transport = HttpTransport::new(&url, &HashMap::new()).unwrap();
        transport.request("initialize", Some(json!({}))).await.unwrap();
        assert!(transport.request("tools/call", None).await.unwrap_err().to_string().contains("not retried"));
        let requests = task.await.unwrap();
        assert_eq!(requests.iter().filter(|(_, b)| b["method"] == "tools/call").count(), 1);
        assert!(!requests[2].0.contains("mcp-session-id"));
    }
    #[test]
    fn rejects_unsafe_urls_and_reserved_headers() {
        for url in ["file:///tmp/mcp", "http://user:secret@localhost/mcp", "http://localhost/mcp#fragment"] {
            assert!(HttpTransport::new(url, &HashMap::new()).is_err());
        }
        assert!(HttpTransport::new("http://localhost/mcp", &HashMap::from([("Host".into(), "other".into())])).is_err());
    }
    #[tokio::test]
    async fn enforces_size_and_timeout_limits() {
        for oversized in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096]; let _ = socket.read(&mut buffer).await;
                if oversized {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 20000000\r\n\r\n").await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            });
            let mut transport = HttpTransport::new(&url, &HashMap::new()).unwrap();
            transport.client = reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
            let error = transport.request("tools/list", None).await.unwrap_err().to_string();
            assert!(error.contains(if oversized { "16 MiB" } else { "timed out" }), "{error}");
            task.abort();
        }
    }

}
