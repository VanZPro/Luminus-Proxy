use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use luminus_core::{protocol::CanonicalRequest, provider::ProviderContext};
use luminus_protocols::openai::{ChatRequest, ChatResponse};
use luminus_router::{Router as LuminusRouter, RouterError, RouterErrorCategory};
use serde_json::json;

pub async fn chat_completions(
    State(router): State<Arc<LuminusRouter>>,
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
            Json(
                json!({"error": {"type": "unsupported_capability", "message": "Experimental Rust execution does not support stream=true"}}),
            ),
        ));
    }
    let target = router.resolve(&canonical).map_err(router_error)?;
    let context = ProviderContext::new(
        "experimental-rust",
        target.provider.clone(),
        canonical.model.clone(),
    );
    let plan = luminus_router::RoutePlan {
        candidates: vec![luminus_router::RouteCandidate {
            provider: target.provider,
            model: canonical.model.clone(),
            account: None,
        }],
        policy: luminus_router::RoutingPolicy::new(2, true).map_err(router_error)?,
    };
    let response = router
        .execute_plan(&canonical, &plan, &context)
        .await
        .map_err(router_error)?
        .response;
    ChatResponse::try_from(response).map(Json).map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": {"type": "protocol_error", "message": error.to_string()}})),
        )
    })
}

fn router_error(error: RouterError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match error.category() {
        RouterErrorCategory::ProviderNotFound
        | RouterErrorCategory::ModelNotFound
        | RouterErrorCategory::NoEligibleProvider => StatusCode::SERVICE_UNAVAILABLE,
        RouterErrorCategory::UnsupportedCapability => StatusCode::NOT_IMPLEMENTED,
        RouterErrorCategory::ProviderExecution => StatusCode::BAD_GATEWAY,
        RouterErrorCategory::InvalidPolicy => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({"error": {"type": "routing_error", "message": error.to_string()}})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::post};
    use tower::ServiceExt;

    fn empty_router() -> Arc<LuminusRouter> {
        Arc::new(LuminusRouter::new(
            Arc::new(luminus_router::ProviderRegistry::new()),
            None,
        ))
    }

    #[tokio::test]
    async fn stream_requests_are_rejected_before_routing() {
        let app = Router::new()
            .route("/experimental/v1/chat/completions", post(chat_completions))
            .with_state(empty_router());
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
            .with_state(empty_router());
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
