//! Chat adapters for Anthropic, OpenAI, and Ollama, normalized to one message model.

use super::{Msg, ToolCall, ToolDef, Turn};
use crate::settings::Provider;
use reqwest::Client;
use serde_json::{json, Value};

pub struct Call<'a> {
    pub provider: Provider,
    pub model: &'a str,
    pub key: Option<&'a str>,
    pub ollama_url: &'a str,
    pub system: &'a str,
    pub tools: &'a [ToolDef],
    pub messages: &'a [Msg],
}

pub async fn chat(http: &Client, c: &Call<'_>) -> Result<Turn, String> {
    if c.model.trim().is_empty() {
        return Err("No model selected. Choose one in Settings.".into());
    }
    match c.provider {
        Provider::Anthropic => anthropic(http, c).await,
        Provider::Openai => openai(http, c).await,
        Provider::Ollama => ollama(http, c).await,
    }
}

async fn send(req: reqwest::RequestBuilder, who: &str) -> Result<Value, String> {
    let resp = req.send().await.map_err(|e| format!("{who}: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("{who}: bad response ({e})"))?;
    if !status.is_success() {
        let msg = body
            .pointer("/error/message")
            .or_else(|| body.pointer("/error"))
            .map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))
            .unwrap_or_else(|| body.to_string());
        return Err(format!("{who} {status}: {msg}"));
    }
    Ok(body)
}

/// Base URLs can be overridden (proxies, gateways, tests).
fn base(var: &str, default: &str) -> String {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string()).trim_end_matches('/').to_string()
}
fn anthropic_base() -> String {
    base("MIXLENS_ANTHROPIC_BASE_URL", "https://api.anthropic.com")
}
fn openai_base() -> String {
    base("MIXLENS_OPENAI_BASE_URL", "https://api.openai.com")
}

fn key<'a>(c: &'a Call<'_>, name: &str) -> Result<&'a str, String> {
    c.key.filter(|k| !k.is_empty()).ok_or_else(|| format!("No {name} API key. Add one in Settings."))
}

// ---------------- Anthropic Messages API ----------------

async fn anthropic(http: &Client, c: &Call<'_>) -> Result<Turn, String> {
    let k = key(c, "Anthropic")?;
    let mut msgs: Vec<Value> = vec![];
    for m in c.messages {
        match m {
            Msg::User(t) => msgs.push(json!({"role": "user", "content": t})),
            Msg::Assistant { text, calls } => {
                let mut content = vec![];
                if !text.is_empty() {
                    content.push(json!({"type": "text", "text": text}));
                }
                for tc in calls {
                    content.push(json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.args}));
                }
                msgs.push(json!({"role": "assistant", "content": content}));
            }
            Msg::Tool(results) => {
                let content: Vec<Value> = results
                    .iter()
                    .map(|r| json!({"type": "tool_result", "tool_use_id": r.id, "content": r.content, "is_error": r.is_error}))
                    .collect();
                msgs.push(json!({"role": "user", "content": content}));
            }
        }
    }
    let mut body = json!({
        "model": c.model,
        "max_tokens": 4096,
        // Cache the static system prompt (and the tool list before it) across turns.
        "system": [{"type": "text", "text": c.system, "cache_control": {"type": "ephemeral"}}],
        "messages": msgs,
    });
    if !c.tools.is_empty() {
        body["tools"] = json!(c
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}))
            .collect::<Vec<_>>());
    }
    let v = send(
        http.post(format!("{}/v1/messages", anthropic_base()))
            .header("x-api-key", k)
            .header("anthropic-version", "2023-06-01")
            .json(&body),
        "Anthropic",
    )
    .await?;
    let mut text = String::new();
    let mut calls = vec![];
    for b in v["content"].as_array().cloned().unwrap_or_default() {
        match b["type"].as_str() {
            Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
            Some("tool_use") => calls.push(ToolCall {
                id: b["id"].as_str().unwrap_or_default().to_string(),
                name: b["name"].as_str().unwrap_or_default().to_string(),
                args: b["input"].clone(),
            }),
            _ => {}
        }
    }
    Ok(Turn { text, calls })
}

// ---------------- OpenAI Chat Completions ----------------

fn openai_style_messages(c: &Call<'_>, ollama: bool) -> Vec<Value> {
    let mut msgs = vec![json!({"role": "system", "content": c.system})];
    for m in c.messages {
        match m {
            Msg::User(t) => msgs.push(json!({"role": "user", "content": t})),
            Msg::Assistant { text, calls } => {
                let mut o = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    o["tool_calls"] = json!(calls
                        .iter()
                        .map(|tc| if ollama {
                            json!({"function": {"name": tc.name, "arguments": tc.args}})
                        } else {
                            json!({"id": tc.id, "type": "function", "function": {"name": tc.name, "arguments": tc.args.to_string()}})
                        })
                        .collect::<Vec<_>>());
                }
                msgs.push(o);
            }
            Msg::Tool(results) => {
                for r in results {
                    if ollama {
                        msgs.push(json!({"role": "tool", "content": r.content, "tool_name": r.name}));
                    } else {
                        msgs.push(json!({"role": "tool", "tool_call_id": r.id, "content": r.content}));
                    }
                }
            }
        }
    }
    msgs
}

fn openai_tools(c: &Call<'_>) -> Value {
    json!(c
        .tools
        .iter()
        .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
        .collect::<Vec<_>>())
}

async fn openai(http: &Client, c: &Call<'_>) -> Result<Turn, String> {
    let k = key(c, "OpenAI")?;
    let mut body = json!({"model": c.model, "messages": openai_style_messages(c, false)});
    if !c.tools.is_empty() {
        body["tools"] = openai_tools(c);
    }
    let v = send(http.post(format!("{}/v1/chat/completions", openai_base())).bearer_auth(k).json(&body), "OpenAI").await?;
    let m = &v["choices"][0]["message"];
    let calls = m["tool_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|tc| ToolCall {
            id: tc["id"].as_str().unwrap_or_default().to_string(),
            name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
            args: serde_json::from_str(tc["function"]["arguments"].as_str().unwrap_or("{}")).unwrap_or(json!({})),
        })
        .collect();
    Ok(Turn { text: m["content"].as_str().unwrap_or("").to_string(), calls })
}

// ---------------- Ollama (local) ----------------

async fn ollama(http: &Client, c: &Call<'_>) -> Result<Turn, String> {
    let url = format!("{}/api/chat", c.ollama_url.trim_end_matches('/'));
    let mut body = json!({
        "model": c.model,
        "messages": openai_style_messages(c, true),
        "stream": false,
        "options": {"temperature": 0.2}
    });
    if !c.tools.is_empty() {
        body["tools"] = openai_tools(c);
    }
    let v = send(http.post(&url).json(&body), "Ollama")
        .await
        .map_err(|e| format!("{e}. Is Ollama running at {}?", c.ollama_url))?;
    let m = &v["message"];
    let text = m["content"].as_str().unwrap_or("").to_string();
    let calls: Vec<ToolCall> = m["tool_calls"].as_array().map(|a| ollama_calls(a)).unwrap_or_default();
    if calls.is_empty() {
        // Some local servers (e.g. KoboldCpp's Ollama emulation) and models emit the tool call
        // as plain text instead of structured tool_calls.
        if let Some(calls) = text_tool_calls(&text, c.tools) {
            return Ok(Turn { text: String::new(), calls });
        }
    }
    Ok(Turn { text, calls })
}

fn ollama_calls(arr: &[Value]) -> Vec<ToolCall> {
    arr.iter()
        .enumerate()
        .map(|(i, tc)| {
            // Accept both {"function": {name, arguments}} and bare {name, arguments|parameters}.
            let f = if tc["function"].is_object() { &tc["function"] } else { tc };
            let args = if f["arguments"].is_null() { &f["parameters"] } else { &f["arguments"] };
            ToolCall {
                id: tc["id"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| format!("call_{i}")),
                name: f["name"].as_str().unwrap_or_default().to_string(),
                args: match args {
                    Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                    Value::Null => json!({}),
                    a => a.clone(),
                },
            }
        })
        .collect()
}

/// Recover tool calls written into the reply text. Only accepted when the whole reply is the
/// call (optionally in a ```json fence or <tool_call> tags) and every call names a known tool,
/// so ordinary answers that merely mention JSON are left alone.
pub(crate) fn text_tool_calls(text: &str, tools: &[ToolDef]) -> Option<Vec<ToolCall>> {
    let mut s = text.trim();
    if let Some(rest) = s.strip_prefix("```") {
        s = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic()).trim().strip_suffix("```")?.trim();
    }
    let values: Vec<Value> = if s.starts_with("<tool_call>") {
        s.split("<tool_call>")
            .map(|p| p.trim().trim_end_matches("</tool_call>").trim())
            .filter(|p| !p.is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .ok()?
    } else {
        match serde_json::from_str::<Value>(s).ok()? {
            Value::Array(a) => a,
            Value::Object(o) => match o.get("tool_calls") {
                Some(Value::Array(a)) => a.clone(),
                _ => vec![Value::Object(o)],
            },
            _ => return None,
        }
    };
    let calls = ollama_calls(&values);
    let known = |n: &str| tools.iter().any(|t| t.name == n);
    (!calls.is_empty() && calls.iter().all(|c| known(&c.name))).then_some(calls)
}

// ---------------- model lists ----------------

pub async fn list_models(http: &Client, p: Provider, key: Option<&str>, ollama_url: &str) -> Result<Vec<String>, String> {
    let mut ids: Vec<String> = match p {
        Provider::Anthropic => {
            let k = key.ok_or("No Anthropic API key")?;
            let v = send(
                http.get(format!("{}/v1/models?limit=100", anthropic_base())).header("x-api-key", k).header("anthropic-version", "2023-06-01"),
                "Anthropic",
            )
            .await?;
            v["data"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()
        }
        Provider::Openai => {
            let k = key.ok_or("No OpenAI API key")?;
            let v = send(http.get(format!("{}/v1/models", openai_base())).bearer_auth(k), "OpenAI").await?;
            v["data"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                // Chat-capable families only.
                .filter(|id| {
                    (id.starts_with("gpt-") || id.starts_with('o'))
                        && !["audio", "realtime", "tts", "transcribe", "image", "embedding", "search"].iter().any(|x| id.contains(x))
                })
                .collect()
        }
        Provider::Ollama => {
            let v = send(http.get(format!("{}/api/tags", ollama_url.trim_end_matches('/'))), "Ollama")
                .await
                .map_err(|e| format!("{e}. Is Ollama running?"))?;
            v["models"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| m["name"].as_str().map(String::from)).collect()
        }
    };
    ids.sort();
    ids.dedup();
    Ok(ids)
}
