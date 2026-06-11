pub fn module_name() -> &'static str {
    "run"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}
