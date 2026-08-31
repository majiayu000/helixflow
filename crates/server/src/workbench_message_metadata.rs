use helixflow_agent::{TurnClassification, TurnModeSource};
use serde_json::json;

pub(crate) fn turn_metadata_json(classification: TurnClassification) -> String {
    match classification.source {
        TurnModeSource::Explicit | TurnModeSource::Model => json!({
            "turnMode": classification.mode,
            "turnModeSource": classification.source
        })
        .to_string(),
    }
}
