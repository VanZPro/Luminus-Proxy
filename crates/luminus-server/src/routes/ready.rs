use crate::ExperimentalRuntimeDiagnostics;
use axum::{Extension, Json};
use std::sync::Arc;

pub async fn ready(
    Extension(diagnostics): Extension<Arc<ExperimentalRuntimeDiagnostics>>,
) -> Json<ExperimentalRuntimeDiagnostics> {
    Json((*diagnostics).clone())
}
