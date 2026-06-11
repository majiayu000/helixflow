pub fn module_name() -> &'static str {
    "agent"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
}
