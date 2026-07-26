//! Deterministic capability/model/binding resolution (GH130 T1).
//!
//! Resolution follows the fixed order from `specs/GH130/tech.md` §5 and is
//! fail-closed (P8): unknown, ambiguous, or unavailable results are typed
//! errors — never a silent fallback to a provider default. Model name
//! normalization is exact-match against declared ids, display names, and
//! aliases; substring or "closest model" matching is forbidden.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::{
    BindingAvailability, CapabilityBinding, CatalogSnapshot, ImplementationTarget,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveRequest {
    pub capability_id: String,
    #[serde(default)]
    pub requested_model: Option<String>,
    /// Restricts binding selection to one connector (tech.md §5 input
    /// `backend_preference`): used at run time so the workspace-selected
    /// provider only executes bindings it actually implements.
    #[serde(default)]
    pub connector_preference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedImplementation {
    pub capability_id: String,
    pub requested_model_id: Option<String>,
    pub resolved_model_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub target: ImplementationTarget,
}

/// Runtime availability of connectors as seen by the caller's workspace.
/// Fail-closed: a connector missing from the map counts as unavailable.
pub type ConnectorAvailability = BTreeMap<String, bool>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    CapabilityNotFound {
        capability_id: String,
    },
    ModelNotFound {
        query: String,
    },
    ModelAmbiguous {
        query: String,
        candidates: Vec<String>,
    },
    BindingNotFound {
        capability_id: String,
        model_id: Option<String>,
    },
    BindingAmbiguous {
        capability_id: String,
        candidates: Vec<String>,
    },
    BindingUnavailable {
        capability_id: String,
        binding_ids: Vec<String>,
    },
}

impl ResolveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CapabilityNotFound { .. } => "CAPABILITY_NOT_FOUND",
            Self::ModelNotFound { .. } => "MODEL_NOT_FOUND",
            Self::ModelAmbiguous { .. } => "MODEL_AMBIGUOUS",
            Self::BindingNotFound { .. } => "BINDING_NOT_FOUND",
            Self::BindingAmbiguous { .. } => "BINDING_AMBIGUOUS",
            Self::BindingUnavailable { .. } => "BINDING_UNAVAILABLE",
        }
    }

    /// Recoverable errors should route to clarification instead of a hard
    /// failure (`clarify_first` in the agent contract).
    pub fn recoverable(&self) -> bool {
        matches!(
            self,
            Self::ModelAmbiguous { .. }
                | Self::BindingAmbiguous { .. }
                | Self::BindingUnavailable { .. }
        )
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapabilityNotFound { capability_id } => {
                write!(f, "capability `{capability_id}` is not in the catalog")
            }
            Self::ModelNotFound { query } => {
                write!(f, "no catalog model matches `{query}`")
            }
            Self::ModelAmbiguous { query, candidates } => {
                write!(
                    f,
                    "model name `{query}` matches multiple models: {}",
                    candidates.join(", ")
                )
            }
            Self::BindingNotFound {
                capability_id,
                model_id,
            } => match model_id {
                Some(model_id) => write!(
                    f,
                    "no binding implements capability `{capability_id}` with model `{model_id}`"
                ),
                None => write!(f, "no binding implements capability `{capability_id}`"),
            },
            Self::BindingAmbiguous {
                capability_id,
                candidates,
            } => write!(
                f,
                "multiple bindings for capability `{capability_id}` and no configured default: {}",
                candidates.join(", ")
            ),
            Self::BindingUnavailable {
                capability_id,
                binding_ids,
            } => write!(
                f,
                "all bindings for capability `{capability_id}` are unavailable: {}",
                binding_ids.join(", ")
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

pub struct CapabilityResolver<'a> {
    catalog: &'a CatalogSnapshot,
}

impl<'a> CapabilityResolver<'a> {
    pub fn new(catalog: &'a CatalogSnapshot) -> Self {
        Self { catalog }
    }

    pub fn resolve(
        &self,
        request: &ResolveRequest,
        availability: &ConnectorAvailability,
    ) -> Result<ResolvedImplementation, ResolveError> {
        let capability_id = request.capability_id.as_str();
        if self.catalog.capability(capability_id).is_none() {
            return Err(ResolveError::CapabilityNotFound {
                capability_id: capability_id.to_owned(),
            });
        }

        let preference = request.connector_preference.as_deref();
        match request.requested_model.as_deref() {
            Some(query) => self.resolve_pinned(capability_id, query, preference, availability),
            None => self.resolve_policy(capability_id, preference, availability),
        }
    }

    fn resolve_pinned(
        &self,
        capability_id: &str,
        query: &str,
        preference: Option<&str>,
        availability: &ConnectorAvailability,
    ) -> Result<ResolvedImplementation, ResolveError> {
        let model_id = self.normalize_model(query)?;
        let mut candidates = self.catalog.bindings_for_pair(capability_id, &model_id);
        if let Some(preference) = preference {
            candidates.retain(|binding| binding_connector(binding) == Some(preference));
        }
        if candidates.is_empty() {
            return Err(ResolveError::BindingNotFound {
                capability_id: capability_id.to_owned(),
                model_id: Some(model_id),
            });
        }

        let binding = self.select_unique(capability_id, candidates, availability)?;
        Ok(resolved(capability_id, Some(model_id), binding))
    }

    fn resolve_policy(
        &self,
        capability_id: &str,
        preference: Option<&str>,
        availability: &ConnectorAvailability,
    ) -> Result<ResolvedImplementation, ResolveError> {
        let mut candidates = self.catalog.bindings_for_capability(capability_id);
        if let Some(preference) = preference {
            candidates.retain(|binding| binding_connector(binding) == Some(preference));
        }
        if candidates.is_empty() {
            return Err(ResolveError::BindingNotFound {
                capability_id: capability_id.to_owned(),
                model_id: None,
            });
        }

        // Policy selection uses the explicitly configured default (P5). With
        // a connector preference the default may live on another connector;
        // then a unique preferred candidate is still an explicit choice —
        // multiple candidates remain ambiguous.
        let default_binding =
            self.catalog
                .default_bindings
                .get(capability_id)
                .and_then(|default_id| {
                    candidates
                        .iter()
                        .find(|binding| &binding.binding_id == default_id)
                        .copied()
                });
        let binding = match default_binding {
            Some(binding) => binding,
            None if preference.is_some() && candidates.len() == 1 => candidates[0],
            None => {
                return Err(ResolveError::BindingAmbiguous {
                    capability_id: capability_id.to_owned(),
                    candidates: binding_ids(&candidates),
                });
            }
        };
        if !self.binding_available(binding, availability) {
            return Err(ResolveError::BindingUnavailable {
                capability_id: capability_id.to_owned(),
                binding_ids: vec![binding.binding_id.clone()],
            });
        }
        Ok(resolved(capability_id, None, binding))
    }

    /// Normalizes a user-supplied model name to exactly one canonical model
    /// id. Matching is exact (after whitespace/hyphen normalization) against
    /// model id, id tail, display name, and declared aliases.
    fn normalize_model(&self, query: &str) -> Result<String, ResolveError> {
        let needle = normalize_name(query);
        if needle.is_empty() {
            return Err(ResolveError::ModelNotFound {
                query: query.to_owned(),
            });
        }

        let mut matches: Vec<String> = self
            .catalog
            .models
            .iter()
            .filter(|model| {
                let mut names = vec![
                    normalize_name(&model.model_id),
                    normalize_name(&model.display_name),
                ];
                if let Some((_, tail)) = model.model_id.split_once('/') {
                    names.push(normalize_name(tail));
                }
                names.extend(model.aliases.iter().map(|alias| normalize_name(alias)));
                names.contains(&needle)
            })
            .map(|model| model.model_id.clone())
            .collect();
        matches.sort_unstable();
        matches.dedup();

        match matches.len() {
            0 => Err(ResolveError::ModelNotFound {
                query: query.to_owned(),
            }),
            1 => Ok(matches.remove(0)),
            _ => Err(ResolveError::ModelAmbiguous {
                query: query.to_owned(),
                candidates: matches,
            }),
        }
    }

    /// Picks exactly one binding from the candidates for a pinned model: the
    /// configured capability default wins if it is among them and available;
    /// otherwise the available set must be a singleton.
    fn select_unique(
        &self,
        capability_id: &str,
        candidates: Vec<&'a CapabilityBinding>,
        availability: &ConnectorAvailability,
    ) -> Result<&'a CapabilityBinding, ResolveError> {
        let all_ids = binding_ids(&candidates);
        let available: Vec<&CapabilityBinding> = candidates
            .into_iter()
            .filter(|binding| self.binding_available(binding, availability))
            .collect();
        if available.is_empty() {
            return Err(ResolveError::BindingUnavailable {
                capability_id: capability_id.to_owned(),
                binding_ids: all_ids,
            });
        }
        if let Some(default_id) = self.catalog.default_bindings.get(capability_id)
            && let Some(binding) = available
                .iter()
                .find(|binding| &binding.binding_id == default_id)
        {
            return Ok(binding);
        }
        if available.len() == 1 {
            return Ok(available[0]);
        }
        Err(ResolveError::BindingAmbiguous {
            capability_id: capability_id.to_owned(),
            candidates: binding_ids(&available),
        })
    }

    fn binding_available(
        &self,
        binding: &CapabilityBinding,
        availability: &ConnectorAvailability,
    ) -> bool {
        if binding.availability != BindingAvailability::Enabled {
            return false;
        }
        match &binding.implementation {
            ImplementationTarget::ApiConnector { connector_id, .. } => {
                let declared = self
                    .catalog
                    .connector(connector_id)
                    .is_some_and(|connector| connector.enabled);
                declared && availability.get(connector_id).copied().unwrap_or(false)
            }
            // No workflow backend ships in V1; fail closed until one does.
            ImplementationTarget::WorkflowTemplate { .. } => false,
        }
    }
}

fn resolved(
    capability_id: &str,
    requested_model_id: Option<String>,
    binding: &CapabilityBinding,
) -> ResolvedImplementation {
    ResolvedImplementation {
        capability_id: capability_id.to_owned(),
        requested_model_id,
        resolved_model_id: binding.model_id.clone(),
        binding_id: binding.binding_id.clone(),
        binding_revision: binding.binding_revision.clone(),
        target: binding.implementation.clone(),
    }
}

fn binding_connector(binding: &CapabilityBinding) -> Option<&str> {
    match &binding.implementation {
        ImplementationTarget::ApiConnector { connector_id, .. } => Some(connector_id),
        ImplementationTarget::WorkflowTemplate { .. } => None,
    }
}

fn binding_ids(bindings: &[&CapabilityBinding]) -> Vec<String> {
    bindings
        .iter()
        .map(|binding| binding.binding_id.clone())
        .collect()
}

fn normalize_name(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut last_was_space = true;
    for ch in value.chars() {
        let ch = match ch {
            '-' | '_' | '/' | '.' => ' ',
            other => other,
        };
        if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
        } else {
            for lower in ch.to_lowercase() {
                normalized.push(lower);
            }
            last_was_space = false;
        }
    }
    normalized.trim_end().to_owned()
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
