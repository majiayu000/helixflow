pub fn module_name() -> &'static str {
    "store"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "store");
    }

    #[test]
    fn serializes_workspace_record_boundary() {
        let workspace = WorkspaceRecord {
            id: "workspace-1".to_string(),
            name: "Demo workspace".to_string(),
        };

        let encoded = serde_json::to_value(&workspace).expect("serialize workspace");

        assert_eq!(encoded["id"], "workspace-1");
        assert_eq!(encoded["name"], "Demo workspace");
    }
}
