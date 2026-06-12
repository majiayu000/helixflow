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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "run");
    }

    #[test]
    fn serializes_run_status_boundary() {
        let encoded = serde_json::to_value(RunStatus::Interrupted).expect("serialize run status");

        assert_eq!(encoded, "Interrupted");
    }
}
