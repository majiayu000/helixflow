pub fn module_name() -> &'static str {
    "store"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
}
