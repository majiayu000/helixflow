use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    Chat,
    CreateWorkflow,
    ModifyWorkflow,
    DebugWorkflow,
    RunRequest,
    /// Internal model-owned semantic routing turn. HTTP callers cannot
    /// deserialize this control-plane mode.
    #[serde(skip_deserializing)]
    Route,
}

impl TurnMode {
    pub fn output_contract(self) -> OutputContract {
        match self {
            Self::Chat => OutputContract::ReplyJson,
            Self::CreateWorkflow | Self::ModifyWorkflow | Self::DebugWorkflow => {
                OutputContract::CanvasEditJson
            }
            Self::RunRequest => OutputContract::RunRequestJson,
            Self::Route => OutputContract::RouteJson,
        }
    }

    pub fn agent_skill(self) -> AgentSkill {
        match self {
            Self::Chat => AgentSkill::Chat,
            Self::CreateWorkflow => AgentSkill::CreateWorkflow,
            Self::ModifyWorkflow => AgentSkill::ModifyWorkflow,
            Self::DebugWorkflow => AgentSkill::FixError,
            Self::RunRequest => AgentSkill::RunRequest,
            Self::Route => AgentSkill::Route,
        }
    }

    pub fn uses_graph_context(self) -> bool {
        !matches!(self, Self::Chat | Self::Route)
    }

    pub fn uses_canvas_context(self) -> bool {
        !matches!(self, Self::Route)
    }
}

impl fmt::Display for TurnMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Chat => "chat",
            Self::CreateWorkflow => "create_workflow",
            Self::ModifyWorkflow => "modify_workflow",
            Self::DebugWorkflow => "debug_workflow",
            Self::RunRequest => "run_request",
            Self::Route => "route",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputContract {
    ReplyJson,
    CanvasEditJson,
    RunRequestJson,
    RouteJson,
}

impl OutputContract {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::ReplyJson => "reply.json",
            Self::CanvasEditJson => "canvas_edit.json",
            Self::RunRequestJson => "run_request.json",
            Self::RouteJson => "route.json",
        }
    }
}

impl fmt::Display for OutputContract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ReplyJson => "reply_json",
            Self::CanvasEditJson => "canvas_edit_json",
            Self::RunRequestJson => "run_request_json",
            Self::RouteJson => "route_json",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSkill {
    Chat,
    CreateWorkflow,
    ModifyWorkflow,
    FixError,
    RunRequest,
    Sweep,
    Route,
}

impl AgentSkill {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Chat => "chat.md",
            Self::CreateWorkflow => "create_workflow.md",
            Self::ModifyWorkflow => "modify_workflow.md",
            Self::FixError => "fix_error.md",
            Self::RunRequest => "run_request.md",
            Self::Sweep => "sweep.md",
            Self::Route => "route.md",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnModeSource {
    Explicit,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnClassification {
    pub mode: TurnMode,
    pub source: TurnModeSource,
}

/// Uses an explicit UI surface intent instead of guessing from message text.
/// The server only calls this after deserializing a known [`TurnMode`], so an
/// unsupported override is rejected at the HTTP boundary.
pub fn explicit_turn_mode(mode: TurnMode) -> TurnClassification {
    TurnClassification {
        mode,
        source: TurnModeSource::Explicit,
    }
}
