use super::cost_types::CostSummary;
use super::{RunError, RunResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentFixPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
}

pub fn agent_fix_policy() -> RunResult<AgentFixPolicy> {
    let enabled = match std::env::var("HELIXFLOW_RUN_AGENT_FIX_ENABLED") {
        Ok(value) => parse_agent_fix_enabled(Some(&value))?,
        Err(std::env::VarError::NotPresent) => false,
        Err(std::env::VarError::NotUnicode(_)) => return Err(fix_enabled_error()),
    };
    if !enabled {
        return Ok(AgentFixPolicy {
            enabled: false,
            max_attempts: 0,
        });
    }
    let max_attempts = match std::env::var("HELIXFLOW_RUN_MAX_FIX_ATTEMPTS") {
        Ok(value) => parse_max_fix_attempts(Some(&value))?,
        Err(std::env::VarError::NotPresent) => parse_max_fix_attempts(None)?,
        Err(std::env::VarError::NotUnicode(_)) => return Err(fix_limit_error()),
    };
    Ok(AgentFixPolicy {
        enabled,
        max_attempts,
    })
}

pub fn parse_agent_fix_enabled(raw: Option<&str>) -> RunResult<bool> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None | Some("0" | "false" | "off") => Ok(false),
        Some("1" | "true" | "on") => Ok(true),
        Some(_) => Err(fix_enabled_error()),
    }
}

pub fn parse_max_fix_attempts(raw: Option<&str>) -> RunResult<u32> {
    raw.unwrap_or("1")
        .parse::<u32>()
        .map_err(|_| fix_limit_error())
}

fn fix_enabled_error() -> RunError {
    RunError::InvalidConfiguration(
        "HELIXFLOW_RUN_AGENT_FIX_ENABLED must be true, false, on, off, 1, or 0".to_owned(),
    )
}

fn fix_limit_error() -> RunError {
    RunError::InvalidConfiguration(
        "HELIXFLOW_RUN_MAX_FIX_ATTEMPTS must be a non-negative integer".to_owned(),
    )
}

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
    // Unknown totals must never be treated as free (HF-004).
    if cost.unknown || !cost.amount.is_finite() || cost.amount < 0.0 {
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
pub fn max_run_retries() -> RunResult<u32> {
    match std::env::var("HELIXFLOW_RUN_MAX_RETRIES") {
        Ok(value) => parse_max_run_retries(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_max_run_retries(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(retry_limit_error()),
    }
}

pub fn parse_max_run_retries(raw: Option<&str>) -> RunResult<u32> {
    let Some(raw) = raw else {
        return Ok(1);
    };
    raw.parse::<u32>().map_err(|_| retry_limit_error())
}

fn retry_limit_error() -> RunError {
    RunError::InvalidConfiguration(
        "HELIXFLOW_RUN_MAX_RETRIES must be a non-negative integer".to_owned(),
    )
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

    #[test]
    fn strict_retry_limit_parser_distinguishes_missing_and_invalid_values() {
        assert_eq!(parse_max_run_retries(None).expect("default retry limit"), 1);
        assert_eq!(
            parse_max_run_retries(Some("0")).expect("disabled retries"),
            0
        );
        assert_eq!(parse_max_run_retries(Some("7")).expect("retry limit"), 7);
        assert_eq!(
            parse_max_run_retries(Some(&u32::MAX.to_string())).expect("u32 max retry limit"),
            u32::MAX
        );
        for invalid in ["", "-1", "1.5", "not-a-number", "4294967296"] {
            assert!(
                parse_max_run_retries(Some(invalid)).is_err(),
                "invalid retry limit `{invalid}` must fail closed"
            );
        }
    }

    #[test]
    fn agent_fix_policy_parsers_are_strict_and_default_off() {
        assert!(!parse_agent_fix_enabled(None).expect("default off"));
        for enabled in ["1", "true", "on", " TRUE "] {
            assert!(parse_agent_fix_enabled(Some(enabled)).expect("enabled value"));
        }
        for disabled in ["0", "false", "off", " OFF "] {
            assert!(!parse_agent_fix_enabled(Some(disabled)).expect("disabled value"));
        }
        for invalid in ["", "yes", "-1", "enabled"] {
            assert!(parse_agent_fix_enabled(Some(invalid)).is_err());
        }
        assert_eq!(parse_max_fix_attempts(None).expect("default limit"), 1);
        assert_eq!(parse_max_fix_attempts(Some("0")).expect("zero limit"), 0);
        assert_eq!(parse_max_fix_attempts(Some("7")).expect("limit"), 7);
        for invalid in ["", "-1", "1.5", "4294967296"] {
            assert!(parse_max_fix_attempts(Some(invalid)).is_err());
        }
    }
}
