pub fn module_name() -> &'static str {
    "registry"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeDefinition {
    pub node_type: String,
    pub title: String,
    pub category: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "registry");
    }

    #[test]
    fn serializes_node_definition_boundary() {
        let definition = NodeDefinition {
            node_type: "mock.image".to_string(),
            title: "Mock Image".to_string(),
            category: "mock".to_string(),
        };

        let encoded = serde_json::to_value(&definition).expect("serialize node definition");

        assert_eq!(encoded["node_type"], "mock.image");
        assert_eq!(encoded["title"], "Mock Image");
        assert_eq!(encoded["category"], "mock");
    }
}
