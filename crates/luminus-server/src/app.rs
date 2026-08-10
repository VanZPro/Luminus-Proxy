use axum::{
    Router,
    routing::{get, post},
};
use luminus_router::Router as LuminusRouter;
use std::sync::Arc;

use crate::routes::health::health;
use crate::routes::openai::chat_completions;

pub fn experimental_app(router: Arc<LuminusRouter>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/experimental/v1/chat/completions", post(chat_completions))
        .with_state(router)
}
