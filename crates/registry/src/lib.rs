pub fn module_name() -> &'static str {
    "registry"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeDefinition {
    pub node_type: String,
    pub title: String,
    pub category: String,
}
