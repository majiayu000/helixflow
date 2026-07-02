use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::RuntimeEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLogEntry {
    pub kind: String,
    pub label: String,
    pub text: String,
    pub raw_json: String,
}

pub async fn read_runtime_stdout<R>(
    stdout: R,
    sender: mpsc::Sender<RuntimeEvent>,
    transcript_path: PathBuf,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Some(entry) = runtime_log_from_line(&line) else {
            continue;
        };
        if let Err(err) = append_transcript(&transcript_path, &entry).await {
            if sender
                .send(RuntimeEvent::Log {
                    kind: "transcript_error".to_owned(),
                    label: "transcript write failed".to_owned(),
                    text: err.to_string(),
                    raw_json: json!({ "error": err.to_string() }).to_string(),
                })
                .await
                .is_err()
            {
                break;
            }
        }
        if sender
            .send(RuntimeEvent::Log {
                kind: entry.kind,
                label: entry.label,
                text: entry.text,
                raw_json: entry.raw_json,
            })
            .await
            .is_err()
        {
            break;
        }
    }
}

pub fn truncate_status(value: &str) -> String {
    value.chars().take(180).collect()
}

fn runtime_log_from_line(line: &str) -> Option<RuntimeLogEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Some(RuntimeLogEntry {
            kind: "stdout".to_owned(),
            label: "stdout".to_owned(),
            text: line.to_owned(),
            raw_json: json!({ "line": line }).to_string(),
        });
    };
    let item = value.get("item").unwrap_or(&value);
    let kind = item
        .get("type")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("event");
    if is_lifecycle_event(kind) {
        return None;
    }
    let text = item_text(item)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| value.get("text").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| {
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| line.to_owned())
        });
    let label = runtime_label(kind, item);
    let normalized_kind = runtime_kind(kind);
    if is_internal_log_event(&normalized_kind, &text) {
        return None;
    }

    Some(RuntimeLogEntry {
        kind: normalized_kind,
        label,
        text,
        raw_json: serde_json::to_string_pretty(&value).unwrap_or_else(|_| line.to_owned()),
    })
}

fn is_lifecycle_event(kind: &str) -> bool {
    matches!(
        kind,
        "thread.started"
            | "turn.started"
            | "turn.completed"
            | "turn.sent"
            | "response.started"
            | "response.completed"
    )
}

fn runtime_kind(kind: &str) -> String {
    if kind.contains("call") && !kind.contains("output") {
        "tool_call".to_owned()
    } else if kind.contains("output") || kind.contains("result") {
        "tool_result".to_owned()
    } else if kind.contains("message") {
        "assistant_message".to_owned()
    } else {
        kind.replace('.', "_")
    }
}

fn is_internal_log_event(kind: &str, text: &str) -> bool {
    if matches!(kind, "assistant_message" | "file_change") {
        return true;
    }
    kind == "error" && text.contains("Skill descriptions were shortened")
}

fn runtime_label(kind: &str, item: &Value) -> String {
    if let Some(name) = item
        .get("name")
        .or_else(|| item.pointer("/call/name"))
        .and_then(Value::as_str)
    {
        return format!("tool call · {name}");
    }
    if kind.contains("output") || kind.contains("result") {
        return "tool result".to_owned();
    }
    if kind.contains("message") {
        return item
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant")
            .to_owned();
    }
    kind.to_owned()
}

fn item_text(item: &Value) -> Option<String> {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return Some(text.to_owned());
    }
    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
        return Some(arguments.to_owned());
    }
    if let Some(command) = item.get("cmd").or_else(|| item.get("command")) {
        let mut text = value_to_text(command);
        if let Some(output) = item
            .get("aggregated_output")
            .or_else(|| item.get("output"))
            .or_else(|| item.get("result"))
            && !output.is_null()
        {
            let output_text = value_to_text(output);
            if !output_text.trim().is_empty() {
                text.push_str("\n\n");
                text.push_str(&output_text);
            }
        }
        return Some(text);
    }
    if let Some(output) = item.get("output").or_else(|| item.get("result")) {
        return Some(value_to_text(output));
    }
    item.get("content").and_then(content_text)
}

fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .or_else(|| item.get("output_text"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn value_to_text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    })
}

async fn append_transcript(path: &PathBuf, entry: &RuntimeLogEntry) -> std::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    let line = json!({
        "role": "agent",
        "kind": entry.kind,
        "label": entry.label,
        "text": entry.text,
        "raw": entry.raw_json
    });
    file.write_all(line.to_string().as_bytes()).await?;
    file.write_all(b"\n").await
}

#[cfg(test)]
mod tests {
    use super::runtime_log_from_line;

    #[test]
    fn skips_codex_lifecycle_events() {
        assert!(runtime_log_from_line(r#"{"type":"thread.started"}"#).is_none());
        assert!(runtime_log_from_line(r#"{"type":"turn.started"}"#).is_none());
        assert!(
            runtime_log_from_line(r#"{"type":"turn.completed","usage":{"input_tokens":12}}"#)
                .is_none()
        );
    }

    #[test]
    fn keeps_tool_events() {
        let Some(entry) = runtime_log_from_line(
            r#"{"item":{"type":"command_execution","cmd":"/bin/zsh -lc pwd"}}"#,
        ) else {
            panic!("tool event should be retained");
        };

        assert_eq!(entry.kind, "command_execution");
        assert_eq!(entry.label, "command_execution");
        assert!(entry.text.contains("/bin/zsh"));
    }

    #[test]
    fn skips_internal_agent_messages_and_file_changes() {
        assert!(
            runtime_log_from_line(
                r#"{"item":{"type":"assistant_message","text":"I will inspect files now."}}"#
            )
            .is_none()
        );
        assert!(
            runtime_log_from_line(
                r#"{"item":{"type":"file_change","changes":[{"path":"/tmp/session/out/reply.json"}]}}"#
            )
            .is_none()
        );
    }

    #[test]
    fn skips_skill_budget_warning() {
        assert!(
            runtime_log_from_line(
                r#"{"item":{"type":"error","message":"Skill descriptions were shortened to fit the 2% skills context budget."}}"#
            )
            .is_none()
        );
    }
}
