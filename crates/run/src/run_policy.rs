use super::cost_types::CostSummary;
use super::{RunError, RunResult};

/// Reads `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD` (default 0.0).
pub fn run_confirmation_threshold_usd() -> RunResult<f64> {
    match std::env::var("HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD") {
        Ok(value) => parse_run_confirmation_threshold_usd(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_run_confirmation_threshold_usd(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(RunError::InvalidConfiguration(
            "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD must be valid UTF-8".to_owned(),
        )),
    }
}

pub fn parse_run_confirmation_threshold_usd(raw: Option<&str>) -> RunResult<f64> {
    let Some(raw) = raw else {
        return Ok(0.0);
    };
    let value = raw.parse::<f64>().map_err(|_| threshold_error())?;
    if !value.is_finite() || value < 0.0 {
        return Err(threshold_error());
    }
    Ok(value)
}

/// Whether a run with the given estimate must be confirmed before starting.
pub fn run_requires_confirmation(cost: &CostSummary) -> RunResult<bool> {
    if !cost.amount.is_finite() || cost.amount < 0.0 {
        return Ok(true);
    }
    if cost.currency != "USD" {
        return Ok(cost.amount > 0.0);
    }
    Ok(cost.amount > run_confirmation_threshold_usd()?)
}

fn threshold_error() -> RunError {
    RunError::InvalidConfiguration(
        "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD must be a finite non-negative number"
            .to_owned(),
    )
}

/// Maximum automatic retries for failure self-repair.
pub fn max_run_retries() -> u32 {
    std::env::var("HELIXFLOW_RUN_MAX_RETRIES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_threshold_parser_rejects_invalid_values() {
        assert_eq!(
            parse_run_confirmation_threshold_usd(None).expect("default threshold"),
            0.0
        );
        assert_eq!(
            parse_run_confirmation_threshold_usd(Some("1.25")).expect("threshold"),
            1.25
        );
        for invalid in ["", "nan", "inf", "-0.01", "not-a-number"] {
            assert!(
                parse_run_confirmation_threshold_usd(Some(invalid)).is_err(),
                "invalid threshold `{invalid}` must fail closed"
            );
        }
    }
}
