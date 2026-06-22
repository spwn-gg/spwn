//! Parse a session JSONL into its main-line user/assistant turns (chronological),
//! excluding sidechain/subagent traffic.

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    /// "text" | "thinking" | "toolUse" | "toolResult"
    pub kind: String,
    pub text: Option<String>,
    /// Tool name (for toolUse).
    pub name: Option<String>,
    /// Whether a toolResult is an error.
    pub is_error: Option<bool>,
    /// For toolUse: the tool_use id. For toolResult: the matching tool_use_id.
    /// Lets the UI pair a result with its call.
    pub id: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub uuid: String,
    pub parent_uuid: Option<String>,
    /// "user" | "assistant"
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub blocks: Vec<Block>,
}

/// Read and parse a transcript into all main-line user/assistant turns, in file
/// (chronological) order. We render the full conversation rather than only the
/// `parentUuid` leaf-chain, so nothing is dropped if a link points at a skipped
/// entry. (Sidechain/subagent traffic is still excluded.)
pub fn read_transcript(path: &Path) -> Vec<Turn> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut turns: Vec<Turn> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(uuid) = v.get("uuid").and_then(Value::as_str) else {
            continue;
        };
        let parent_uuid = v
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(str::to_string);
        let timestamp = v.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let message = v.get("message");
        let model = message
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let blocks = message.map(parse_blocks).unwrap_or_default();

        turns.push(Turn {
            uuid: uuid.to_string(),
            parent_uuid,
            role: kind.to_string(),
            timestamp: Some(timestamp.to_string()),
            model,
            blocks,
        });
    }
    turns
}

/// Map a `message` object's `content` (string or block list) into display blocks.
fn parse_blocks(message: &Value) -> Vec<Block> {
    let Some(content) = message.get("content") else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(s) = content.as_str() {
        if !s.trim().is_empty() {
            out.push(text_block(s));
        }
        return out;
    }

    if let Some(arr) = content.as_array() {
        for b in arr {
            match b.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        if !t.trim().is_empty() {
                            out.push(text_block(t));
                        }
                    }
                }
                "thinking" => {
                    if let Some(t) = b.get("thinking").and_then(Value::as_str) {
                        out.push(Block {
                            kind: "thinking".into(),
                            text: Some(truncate(t, 8000)),
                            name: None,
                            is_error: None,
                            id: None,
                        });
                    }
                }
                "tool_use" => {
                    let name = b
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    // Keep AskUserQuestion's input intact so the UI can render the
                    // question + options as a card (the rest stay truncated/compact).
                    let cap = if name == "AskUserQuestion" { 20_000 } else { 800 };
                    let input = b
                        .get("input")
                        .map(|i| truncate(&compact(i), cap))
                        .unwrap_or_default();
                    out.push(Block {
                        kind: "toolUse".into(),
                        text: Some(input),
                        name: Some(name),
                        is_error: None,
                        id: b.get("id").and_then(Value::as_str).map(str::to_string),
                    });
                }
                "tool_result" => {
                    let is_error = b.get("is_error").and_then(Value::as_bool);
                    let text = b
                        .get("content")
                        .map(|c| truncate(&tool_result_text(c), 8000));
                    out.push(Block {
                        kind: "toolResult".into(),
                        text,
                        name: None,
                        is_error,
                        id: b.get("tool_use_id").and_then(Value::as_str).map(str::to_string),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

fn text_block(s: &str) -> Block {
    Block {
        kind: "text".into(),
        text: Some(s.to_string()),
        name: None,
        is_error: None,
        id: None,
    }
}

/// tool_result `content` is either a string or a list of `{type:text,text}` blocks.
fn tool_result_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for b in arr {
            if let Some(t) = b.get("text").and_then(Value::as_str) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            }
        }
        return out;
    }
    compact(content)
}

fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
