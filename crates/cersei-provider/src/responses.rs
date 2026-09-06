//! Stateless Responses transport for OpenAI reasoning with function tools.
//! Encrypted reasoning items travel in Thinking signatures, never in the UI.

use crate::*;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use serde_json::{json, Value};
use std::collections::HashMap;

fn input_items(messages: &[Message]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in messages {
        if let MessageContent::Blocks(blocks) = &message.content {
            for block in blocks {
                if let ContentBlock::Thinking { signature, .. } = block {
                    if let Ok(item) = serde_json::from_str::<Value>(signature) {
                        if item["type"] == "reasoning" {
                            items.push(item);
                        }
                    }
                }
            }
        }
        for msg in crate::openai::convert_messages(std::slice::from_ref(message)) {
            let role = msg["role"].as_str().unwrap_or("user");
            if role == "tool" {
                items.push(json!({"type":"function_call_output", "call_id":msg["tool_call_id"], "output":msg["content"]}));
                continue;
            }
            if let Some(content) = msg.get("content").filter(|c| !c.is_null()) {
                let content = if let Some(parts) = content.as_array() {
                    Value::Array(parts.iter().map(|part| {
                        if part["type"] == "image_url" {
                            json!({"type":"input_image", "image_url":part["image_url"]["url"]})
                        } else {
                            json!({"type":if role == "assistant" {"output_text"} else {"input_text"}, "text":part["text"]})
                        }
                    }).collect())
                } else {
                    content.clone()
                };
                items.push(json!({"role":role, "content":content}));
            }
            if let Some(calls) = msg["tool_calls"].as_array() {
                for call in calls {
                    items.push(json!({"type":"function_call", "call_id":call["id"], "name":call["function"]["name"], "arguments":call["function"]["arguments"]}));
                }
            }
        }
    }
    items
}

pub(crate) fn body(request: &CompletionRequest, model: &str) -> Value {
    let mut body = json!({
        "model":model, "input":input_items(&request.messages),
        "stream":true, "store":false, "max_output_tokens":request.max_tokens,
        "include":["reasoning.encrypted_content"],
    });
    if let Some(system) = &request.system {
        body["instructions"] = system.clone().into();
    }
    body["reasoning"] = json!({"summary":"auto"});
    if let Some(effort) = request.options.get::<String>("reasoning_effort") {
        body["reasoning"]["effort"] = effort.into();
    }
    if !request.tools.is_empty() {
        body["tools"] = request.tools.iter().map(|t| json!({
            "type":"function", "name":t.name, "description":t.description,
            "parameters":t.input_schema, "strict":false,
        })).collect();
    }
    body
}

#[derive(Default)]
struct Decoder {
    blocks: HashMap<usize, String>,
    tool_use: bool,
    finished: bool,
}

impl Decoder {
    fn process(&mut self, event: Value) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        let index = event["output_index"].as_u64().unwrap_or(0) as usize;
        match event["type"].as_str().unwrap_or("") {
            "response.created" => out.push(StreamEvent::MessageStart {
                id:event["response"]["id"].as_str().unwrap_or_default().into(),
                model:event["response"]["model"].as_str().unwrap_or_default().into(),
            }),
            "response.output_item.added" => {
                let item = &event["item"];
                let kind = match item["type"].as_str() {
                    Some("message") => "text",
                    Some("reasoning") => "thinking",
                    Some("function_call") => { self.tool_use = true; "tool_use" }
                    _ => return out,
                };
                self.blocks.insert(index, kind.into());
                out.push(StreamEvent::ContentBlockStart {
                    index, block_type:kind.into(),
                    id:item["call_id"].as_str().map(String::from),
                    name:item["name"].as_str().map(String::from),
                });
            }
            "response.output_text.delta" | "response.refusal.delta" => out.push(StreamEvent::TextDelta {
                index, text:event["delta"].as_str().unwrap_or_default().into(),
            }),
            "response.reasoning_summary_text.delta" => out.push(StreamEvent::ThinkingDelta {
                index, thinking:event["delta"].as_str().unwrap_or_default().into(),
            }),
            "response.function_call_arguments.delta" => out.push(StreamEvent::InputJsonDelta {
                index, partial_json:event["delta"].as_str().unwrap_or_default().into(),
            }),
            "response.output_item.done" => {
                if let Some(kind) = self.blocks.remove(&index) {
                    if kind == "thinking" {
                        out.push(StreamEvent::ThinkingSignature { index, signature:event["item"].to_string() });
                    }
                    out.push(StreamEvent::ContentBlockStop { index });
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = &event["response"];
                let stop_reason = if response["status"] == "incomplete" {
                    if response["incomplete_details"]["reason"] == "max_output_tokens" {
                        StopReason::MaxTokens
                    } else { StopReason::ContentFilter }
                } else if self.tool_use { StopReason::ToolUse } else { StopReason::EndTurn };
                let u = &response["usage"];
                out.push(StreamEvent::MessageDelta {
                    stop_reason:Some(stop_reason),
                    usage:Some(Usage {
                        input_tokens:u["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens:u["output_tokens"].as_u64().unwrap_or(0),
                        total_tokens:u["total_tokens"].as_u64().unwrap_or(0),
                        cost_usd:None, provider_usage:u.clone(),
                    }),
                });
                out.push(StreamEvent::MessageStop);
                self.finished = true;
            }
            "response.failed" | "error" => {
                let error = if event["type"] == "response.failed" { &event["response"]["error"] } else { &event };
                out.push(StreamEvent::Error { message:error["message"].as_str().unwrap_or("Responses request failed").into() });
                self.finished = true;
            }
            _ => {}
        }
        out
    }
}

pub(crate) fn complete(client: &reqwest::Client, url: &str, auth: &str, body: Value) -> Result<CompletionStream> {
    let mut source = client.post(url).header("authorization", auth).json(&body)
        .eventsource().map_err(|e| CerseiError::Provider(e.to_string()))?;
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut decoder = Decoder::default();
        loop {
            let next = tokio::select! {
                _ = tx.closed() => break,
                next = source.next() => next,
            };
            match next {
                Some(Ok(Event::Open)) => {}
                Some(Ok(Event::Message(message))) => {
                    let event = match serde_json::from_str(&message.data) {
                        Ok(event) => event,
                        Err(e) => {
                            let _ = tx.send(StreamEvent::Error { message:format!("Invalid Responses event: {e}") }).await;
                            break;
                        }
                    };
                    for event in decoder.process(event) {
                        if tx.send(event).await.is_err() { source.close(); return; }
                    }
                    if decoder.finished { break; }
                }
                Some(Err(e)) => {
                    let message = match e {
                        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                            format!("HTTP {status}: {}", response.text().await.unwrap_or_default())
                        }
                        reqwest_eventsource::Error::StreamEnded => "Responses stream ended before completion".into(),
                        other => other.to_string(),
                    };
                    let _ = tx.send(StreamEvent::Error { message }).await;
                    break;
                }
                None => {
                    let _ = tx.send(StreamEvent::Error { message:"Responses stream ended before completion".into() }).await;
                    break;
                }
            }
        }
        source.close();
    });
    Ok(CompletionStream::new(rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_and_tool_results_survive_a_stream_round_trip() {
        let reasoning = json!({"type":"reasoning", "id":"rs_1", "summary":[], "encrypted_content":"opaque"});
        let events = vec![
            json!({"type":"response.created","response":{"id":"resp_1","model":"gpt-5.6-luna"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":reasoning}),
            json!({"type":"response.reasoning_summary_text.delta","output_index":0,"delta":"Checking."}),
            json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"Read"}}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"file_path\":"}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"\"/tmp/example\"}"}),
            json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call"}}),
            json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":80}}}}),
        ];
        let mut decoder = Decoder::default();
        let mut accumulator = StreamAccumulator::new();
        for event in events {
            for event in decoder.process(event) { accumulator.process_event(event); }
        }
        let response = accumulator.into_response().unwrap();
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.cached_input_tokens(), Some(80));
        let result = Message {
            role:Role::User,
            content:MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id:"call_1".into(), content:ToolResultContent::Text("file contents".into()), is_error:None,
            }]),
            id:None, metadata:None,
        };
        let items = input_items(&[response.message, result]);
        assert_eq!(items[0], reasoning);
        let call = items.iter().find(|i| i["type"] == "function_call").unwrap();
        assert_eq!(call["call_id"], "call_1");
        let args:Value = serde_json::from_str(call["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["file_path"], "/tmp/example");
        assert_eq!(items.last().unwrap(), &json!({"type":"function_call_output","call_id":"call_1","output":"file contents"}));
    }

    #[test]
    fn explicit_and_default_effort_do_not_disable_tools() {
        let mut request = CompletionRequest::new("gpt-5.6-luna");
        request.messages.push(Message::user("Hello"));
        request.options.set("reasoning_effort", "high");
        let encoded = body(&request, &request.model);
        assert_eq!(encoded["reasoning"]["effort"], "high");
        assert_eq!(encoded["store"], false);
        assert!(encoded.get("messages").is_none());
        assert!(encoded.get("max_completion_tokens").is_none());
        request.options = ProviderOptions::default();
        assert!(body(&request, &request.model)["reasoning"].get("effort").is_none());
    }

    #[test]
    fn failure_and_token_limit_are_not_successful_end_turns() {
        let mut decoder = Decoder::default();
        assert!(matches!(decoder.process(json!({"type":"response.failed","response":{"error":{"message":"denied"}}}))[0], StreamEvent::Error { .. }));
        let mut decoder = Decoder::default();
        assert!(matches!(decoder.process(json!({"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}}))[0], StreamEvent::MessageDelta { stop_reason:Some(StopReason::MaxTokens), .. }));
    }
}
