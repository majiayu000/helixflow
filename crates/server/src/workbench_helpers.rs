use std::{fs, path::PathBuf};

use helixflow_graph::ProposalKind;
use helixflow_store::ProposalRecord;

use crate::workbench::{WorkbenchError, WorkbenchResult};

pub(crate) fn ensure_proposal_workspace(
    workspace_id: &str,
    proposal: &ProposalRecord,
) -> WorkbenchResult<()> {
    if proposal.workspace_id == workspace_id {
        return Ok(());
    }
    Err(WorkbenchError::BadRequest(format!(
        "proposal `{}` belongs to workspace `{}`",
        proposal.id, proposal.workspace_id
    )))
}

pub(crate) fn proposal_kind_str(kind: ProposalKind) -> &'static str {
    match kind {
        ProposalKind::Create => "create",
        ProposalKind::Modify => "modify",
        ProposalKind::Fix => "fix",
        ProposalKind::Sweep => "sweep",
    }
}

pub(crate) fn proposal_kind(kind: &str) -> WorkbenchResult<ProposalKind> {
    match kind {
        "create" => Ok(ProposalKind::Create),
        "modify" => Ok(ProposalKind::Modify),
        "fix" => Ok(ProposalKind::Fix),
        "sweep" => Ok(ProposalKind::Sweep),
        _ => Err(WorkbenchError::BadRequest(format!(
            "unknown proposal kind `{kind}`"
        ))),
    }
}

pub(crate) fn resolve_program(program: &str) -> WorkbenchResult<PathBuf> {
    let candidate = PathBuf::from(program);
    if candidate.components().count() > 1 {
        return fs::canonicalize(candidate).map_err(WorkbenchError::Io);
    }

    let Some(path) = std::env::var_os("PATH") else {
        return Err(WorkbenchError::Config(format!(
            "cannot resolve `{program}` because PATH is empty"
        )));
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return fs::canonicalize(candidate).map_err(WorkbenchError::Io);
        }
    }
    Err(WorkbenchError::Config(format!(
        "cannot find `{program}` on PATH"
    )))
}
