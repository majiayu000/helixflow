use helixflow_agent::{TurnClassification, TurnModeSource};
use serde_json::json;

pub(crate) fn turn_metadata_json(classification: TurnClassification) -> String {
    match classification.source {
        TurnModeSource::Keyword => json!({ "turnMode": classification.mode }).to_string(),
        TurnModeSource::AmbiguousFallback => {
            json!({ "turnMode": classification.mode, "turnModeSource": classification.source })
                .to_string()
        }
    }
}
