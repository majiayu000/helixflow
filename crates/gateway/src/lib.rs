pub fn module_name() -> &'static str {
    "gateway"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostEstimate {
    pub amount: f64,
    pub currency: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "gateway");
    }

    #[test]
    fn serializes_cost_estimate_boundary() {
        let estimate = CostEstimate {
            amount: 0.42,
            currency: "USD".to_string(),
        };

        let encoded = serde_json::to_value(&estimate).expect("serialize estimate");

        assert_eq!(encoded["amount"], 0.42);
        assert_eq!(encoded["currency"], "USD");
    }
}
