use axum::{
    Extension, Router,
    routing::{get, post},
};
use luminus_router::Router as LuminusRouter;
use std::sync::Arc;

use crate::ExperimentalRuntimeDiagnostics;
use crate::routes::health::health;
use crate::routes::openai::chat_completions;
use crate::routes::ready::ready;

pub fn experimental_app(router: Arc<LuminusRouter>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/experimental/v1/chat/completions", post(chat_completions))
        .with_state(router)
}

pub fn experimental_app_with_diagnostics(
    router: Arc<LuminusRouter>,
    diagnostics: Arc<ExperimentalRuntimeDiagnostics>,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/experimental/ready", get(ready))
        .route("/experimental/v1/chat/completions", post(chat_completions))
        .layer(Extension(diagnostics))
        .with_state(router)
}
