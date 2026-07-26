use std::fmt;

use helixflow_graph::WorkflowGraph;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    Chat,
    CreateWorkflow,
    ModifyWorkflow,
    DebugWorkflow,
    RunRequest,
}

impl TurnMode {
    pub fn output_contract(self) -> OutputContract {
        self.output_contract_with(false)
    }

    /// GH130 T6: graph-editing turns switch to the IntentPlan contract when
    /// requested; chat and run-request turns are unaffected.
    pub fn output_contract_with(self, use_intent_contract: bool) -> OutputContract {
        match self {
            Self::Chat => OutputContract::ReplyJson,
            Self::CreateWorkflow | Self::ModifyWorkflow | Self::DebugWorkflow => {
                if use_intent_contract {
                    OutputContract::IntentJson
                } else {
                    OutputContract::ProposalJson
                }
            }
            Self::RunRequest => OutputContract::RunRequestJson,
        }
    }

    pub fn agent_skill(self) -> AgentSkill {
        match self {
            Self::Chat => AgentSkill::Chat,
            Self::CreateWorkflow => AgentSkill::CreateWorkflow,
            Self::ModifyWorkflow => AgentSkill::ModifyWorkflow,
            Self::DebugWorkflow => AgentSkill::FixError,
            Self::RunRequest => AgentSkill::RunRequest,
        }
    }

    pub fn uses_graph_context(self) -> bool {
        !matches!(self, Self::Chat)
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
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputContract {
    ReplyJson,
    ProposalJson,
    IntentJson,
    RunRequestJson,
}

impl OutputContract {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::ReplyJson => "reply.json",
            Self::ProposalJson => "proposal.json",
            Self::IntentJson => "intent.json",
            Self::RunRequestJson => "run_request.json",
        }
    }
}

impl fmt::Display for OutputContract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ReplyJson => "reply_json",
            Self::ProposalJson => "proposal_json",
            Self::IntentJson => "intent_json",
            Self::RunRequestJson => "run_request_json",
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRoutingError {
    EmptyMessage,
}

impl fmt::Display for TurnRoutingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMessage => f.write_str("cannot classify an empty agent turn"),
        }
    }
}

impl std::error::Error for TurnRoutingError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnModeSource {
    Keyword,
    AmbiguousFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnClassification {
    pub mode: TurnMode,
    pub source: TurnModeSource,
}

pub fn classify_turn_mode(
    user_message: &str,
    graph: &WorkflowGraph,
) -> Result<TurnClassification, TurnRoutingError> {
    let trimmed = user_message.trim();
    if trimmed.is_empty() {
        return Err(TurnRoutingError::EmptyMessage);
    }

    let normalized = trimmed.to_lowercase();
    if contains_any(&normalized, RUN_KEYWORDS) {
        return Ok(keyword(TurnMode::RunRequest));
    }
    if contains_any(&normalized, DEBUG_KEYWORDS) {
        return Ok(keyword(TurnMode::DebugWorkflow));
    }
    if contains_any(&normalized, CHAT_KEYWORDS) {
        return Ok(keyword(TurnMode::Chat));
    }
    if contains_any(&normalized, MODIFY_KEYWORDS) {
        return Ok(keyword(TurnMode::ModifyWorkflow));
    }
    if contains_any(&normalized, CREATE_KEYWORDS) {
        return Ok(keyword(TurnMode::CreateWorkflow));
    }
    if graph.nodes.is_empty() && contains_any(&normalized, WORKFLOW_NOUNS) {
        return Ok(keyword(TurnMode::CreateWorkflow));
    }

    Ok(TurnClassification {
        mode: TurnMode::Chat,
        source: TurnModeSource::AmbiguousFallback,
    })
}

fn keyword(mode: TurnMode) -> TurnClassification {
    TurnClassification {
        mode,
        source: TurnModeSource::Keyword,
    }
}

fn contains_any(value: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| value.contains(keyword))
}

const CHAT_KEYWORDS: &[&str] = &[
    "你好",
    "您好",
    "你是谁",
    "是什么",
    "解释",
    "说明",
    "区别",
    "compare",
    "explain",
    "hello",
    "hi",
    "who are you",
];

const CREATE_KEYWORDS: &[&str] = &[
    "创建",
    "新建",
    "帮我做",
    "搭一个",
    "做一个",
    "生成一个工作流",
    "文生图",
    "图生图",
    "create",
    "build",
    "make a workflow",
];

const MODIFY_KEYWORDS: &[&str] = &[
    "改",
    "调整",
    "加一个",
    "移除",
    "删除",
    "把",
    "change",
    "set ",
    "add ",
    "remove",
    "modify",
];

const DEBUG_KEYWORDS: &[&str] = &[
    "为什么失败",
    "失败了",
    "报错",
    "修复",
    "debug",
    "fix",
    "failed",
    "error",
];

const RUN_KEYWORDS: &[&str] = &[
    "运行",
    "执行",
    "跑一下",
    "跑当前",
    "提交",
    "出图",
    "生成结果",
    "run current",
    "run the current",
    "execute",
    "queue",
    "render current",
];

const WORKFLOW_NOUNS: &[&str] = &["workflow", "工作流", "graph", "流程"];
