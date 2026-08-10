use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use luminus_core::{
    protocol::CanonicalRequest,
    provider::{ProviderAdapter, ProviderContext},
};
use luminus_protocols::openai::{ChatRequest, ChatResponse};
use luminus_providers::BlackboxProvider;
use serde_json::json;

pub async fn chat_completions(
    State(provider): State<Option<Arc<BlackboxProvider>>>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<serde_json::Value>)> {
    let canonical = CanonicalRequest::try_from(request).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"type": "invalid_request_error", "message": error.to_string()}})),
        )
    })?;
    if canonical.stream {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": {"type": "unsupported_capability", "message": "Experimental Rust execution does not support stream=true"}
            })),
        ));
    }
    let provider = provider.ok_or_else(|| (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": {"type": "configuration_error", "message": "Blackbox provider is not configured"}})),
    ))?;
    let context = ProviderContext::new(
        "experimental-rust",
        provider.provider_id().clone(),
        canonical.model.clone(),
    );
    let response = provider
        .execute(&canonical, &context)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": {"type": "provider_error", "message": error.message}})),
            )
        })?;
    ChatResponse::try_from(response).map(Json).map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": {"type": "protocol_error", "message": error.to_string()}})),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::post};
    use tower::ServiceExt;

    #[tokio::test]
    async fn stream_requests_are_rejected_before_provider_configuration() {
        let app = Router::new()
            .route("/experimental/v1/chat/completions", post(chat_completions))
            .with_state(None::<Arc<BlackboxProvider>>);
        let response = app
            .oneshot(
                Request::post("/experimental/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"test","messages":[],"stream":true}"#,
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("response returns");
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn missing_provider_is_explicit_for_non_streaming_requests() {
        let app = Router::new()
            .route("/experimental/v1/chat/completions", post(chat_completions))
            .with_state(None::<Arc<BlackboxProvider>>);
        let response = app
            .oneshot(
                Request::post("/experimental/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"test","messages":[]}"#))
                    .expect("request builds"),
            )
            .await
            .expect("response returns");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
