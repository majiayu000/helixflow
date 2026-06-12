pub fn module_name() -> &'static str {
    "agent"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "agent");
    }

    #[test]
    fn serializes_agent_session_boundary() {
        let session = AgentSession {
            id: "session-1".to_string(),
            workspace_id: "workspace-1".to_string(),
        };

        let encoded = serde_json::to_value(&session).expect("serialize session");

        assert_eq!(encoded["id"], "session-1");
        assert_eq!(encoded["workspace_id"], "workspace-1");
    }
}
