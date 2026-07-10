use super::cost_gate::CostSummary;

/// Reads `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD` (default 0.0).
pub fn run_confirmation_threshold_usd() -> f64 {
    std::env::var("HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0)
}

/// Whether a run with the given estimate must be confirmed before starting.
pub fn run_requires_confirmation(cost: &CostSummary) -> bool {
    if !cost.amount.is_finite() {
        return true;
    }
    if cost.currency != "USD" {
        return cost.amount > 0.0;
    }
    cost.amount > run_confirmation_threshold_usd()
}

/// Maximum automatic retries for failure self-repair.
pub fn max_run_retries() -> u32 {
    std::env::var("HELIXFLOW_RUN_MAX_RETRIES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1)
}
