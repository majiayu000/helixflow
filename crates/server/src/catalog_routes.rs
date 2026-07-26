//! Capability catalog API (GH130 T1).
//!
//! Serves the catalog snapshot, the two catalog projections, and the
//! deterministic resolve endpoint. Errors carry the stable codes from
//! `specs/GH130/tech.md` §13 and never include credentials or endpoints.

use std::sync::OnceLock;

use axum::Json;
use axum::extract::{Path, State};
use helixflow_gateway::ProviderCatalogSnapshot;
use helixflow_registry::catalog::{CapabilityDefinition, CatalogSnapshot, ModelDefinition};
use helixflow_registry::catalog_seed::builtin_catalog;
use helixflow_registry::resolver::{
    CapabilityResolver, ConnectorAvailability, ResolveError, ResolveRequest, ResolvedImplementation,
};
use serde::Serialize;
use serde_json::json;

use crate::api_error::ApiError;
use crate::app_state::AppState;

pub(crate) fn shared_catalog() -> &'static CatalogSnapshot {
    static CATALOG: OnceLock<CatalogSnapshot> = OnceLock::new();
    CATALOG.get_or_init(builtin_catalog)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityModels {
    capability_id: String,
    models: Vec<ModelDefinition>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCapabilities {
    model_id: String,
    capabilities: Vec<CapabilityDefinition>,
}

pub(crate) async fn catalog_snapshot() -> Json<&'static CatalogSnapshot> {
    Json(shared_catalog())
}

pub(crate) async fn capability_models(
    Path(capability_id): Path<String>,
) -> Result<Json<CapabilityModels>, ApiError> {
    let catalog = shared_catalog();
    if catalog.capability(&capability_id).is_none() {
        return Err(catalog_error(&ResolveError::CapabilityNotFound {
            capability_id,
        }));
    }
    let models = catalog
        .models_for_capability(&capability_id)
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(CapabilityModels {
        capability_id,
        models,
    }))
}

pub(crate) async fn model_capabilities(
    Path(model_id): Path<String>,
) -> Result<Json<ModelCapabilities>, ApiError> {
    let catalog = shared_catalog();
    if catalog.model(&model_id).is_none() {
        return Err(catalog_error(&ResolveError::ModelNotFound {
            query: model_id,
        }));
    }
    let capabilities = catalog
        .capabilities_for_model(&model_id)
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(ModelCapabilities {
        model_id,
        capabilities,
    }))
}

pub(crate) async fn resolve_implementation(
    State(state): State<AppState>,
    Json(request): Json<ResolveRequest>,
) -> Result<Json<ResolvedImplementation>, ApiError> {
    let catalog = shared_catalog();
    let availability = connector_availability(&state.provider_registry.catalog_snapshot());
    let resolver = CapabilityResolver::new(catalog);
    let resolved = resolver
        .resolve(&request, &availability)
        .map_err(|err| catalog_error(&err))?;
    Ok(Json(resolved))
}

/// A connector is available when its provider is enabled and reports healthy.
/// Anything else — including a provider the registry does not know — fails
/// closed to unavailable.
pub(crate) fn connector_availability(snapshot: &ProviderCatalogSnapshot) -> ConnectorAvailability {
    snapshot
        .runtime_providers
        .iter()
        .map(|provider| {
            (
                provider.id.clone(),
                provider.enabled && provider.status == "healthy",
            )
        })
        .collect()
}

fn catalog_error(err: &ResolveError) -> ApiError {
    let details = json!({
        "code": err.code(),
        "recoverable": err.recoverable(),
        "catalogRevision": shared_catalog().catalog_revision,
    });
    if err.recoverable() {
        ApiError::conflict_with_details(err.to_string(), details)
    } else {
        ApiError::not_found_with_details(err.to_string(), details)
    }
}

#[cfg(test)]
#[path = "catalog_routes_tests.rs"]
mod tests;
