use axum::Json;
use helixflow_registry::{NodeRegistry, RegistryCatalog};

pub(crate) async fn node_registry_catalog() -> Json<RegistryCatalog> {
    Json(NodeRegistry::builtin().export_catalog())
}
