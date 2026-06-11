pub fn module_name() -> &'static str {
    "gateway"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostEstimate {
    pub amount: f64,
    pub currency: String,
}
