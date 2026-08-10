use axum::{Router, routing::get};

use crate::routes::health::health;

pub fn app() -> Router {
    Router::new().route("/health", get(health))
}
