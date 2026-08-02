//! Parse a session JSONL into its main-line user/assistant turns (chronological),
//! excluding sidechain/subagent traffic.

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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

/// Read and parse a transcript into the **active** conversation path — the chain
/// of user/assistant turns reachable from the latest turn via `parentUuid`. A
/// `/rewind` branches the JSONL into a tree; following the leaf-chain hides the
/// abandoned branch instead of showing both. Falls back to all turns (file order)
/// if the chain can't be walked. (Sidechain/subagent traffic is excluded.)
pub fn read_transcript(path: &Path) -> Vec<Turn> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };

    // `parentUuid` for every entry (the chain runs through non-turn entries too),
    // plus the renderable turn for each user/assistant entry.
    let mut parent_of: HashMap<String, Option<String>> = HashMap::new();
    let mut turn_of: HashMap<String, Turn> = HashMap::new();
    let mut all_turns: Vec<Turn> = Vec::new();
    let mut last_turn: Option<String> = None;

    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(uuid) = v.get("uuid").and_then(Value::as_str) else {
            continue;
        };
        let parent_uuid = v
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(str::to_string);
        parent_of.insert(uuid.to_string(), parent_uuid.clone());

        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if (kind == "user" || kind == "assistant")
            && !v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
        {
            let timestamp = v.get("timestamp").and_then(Value::as_str).unwrap_or("");
            let message = v.get("message");
            let model = message
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let blocks = message.map(parse_blocks).unwrap_or_default();
            let turn = Turn {
                uuid: uuid.to_string(),
                parent_uuid,
                role: kind.to_string(),
                timestamp: Some(timestamp.to_string()),
                model,
                blocks,
            };
            turn_of.insert(uuid.to_string(), turn.clone());
            all_turns.push(turn);
            last_turn = Some(uuid.to_string());
        }
    }

    // Walk from the latest turn back to the root, collecting the turns on the way.
    let mut path: Vec<Turn> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cur = last_turn;
    while let Some(u) = cur {
        if !seen.insert(u.clone()) {
            break; // cycle guard
        }
        if let Some(t) = turn_of.get(&u) {
            path.push(t.clone());
        }
        cur = parent_of.get(&u).cloned().flatten();
    }
    if path.is_empty() {
        return all_turns;
    }
    path.reverse();
    path
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
