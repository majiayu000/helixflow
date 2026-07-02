use std::{fs, path::Path};

use serde_json::Value;

pub struct AgentTranscriptMessage {
    pub kind: String,
    pub label: String,
    pub text: String,
}

pub fn read_agent_messages(
    data_dir: &Path,
    session_id: &str,
) -> std::io::Result<Vec<AgentTranscriptMessage>> {
    let path = data_dir
        .join("agent-sessions")
        .join(session_id)
        .join("transcript.jsonl");
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            Some(agent_message_from_line(line))
        })
        .filter(|message| !message.text.trim().is_empty())
        .collect())
}

fn agent_message_from_line(line: &str) -> AgentTranscriptMessage {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return AgentTranscriptMessage {
            kind: "transcript_parse_error".to_owned(),
            label: "transcript parse error".to_owned(),
            text: line.to_owned(),
        };
    };
    if value.get("role").and_then(Value::as_str) == Some("user") {
        return AgentTranscriptMessage {
            kind: "user_echo".to_owned(),
            label: "user".to_owned(),
            text: String::new(),
        };
    }
    AgentTranscriptMessage {
        kind: value
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("agent_log")
            .to_owned(),
        label: value
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("agent log")
            .to_owned(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_else(|| line.trim())
            .to_owned(),
    }
}
