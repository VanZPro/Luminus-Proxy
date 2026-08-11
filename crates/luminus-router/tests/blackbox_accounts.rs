use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use luminus_core::{
    model::{AccountDescriptor, AccountId, ModelId, ProviderId},
    protocol::{CanonicalMessage, CanonicalRequest, ContentPart, MessageRole},
    provider::ProviderContext,
};
use luminus_providers::{BlackboxConfig, BlackboxProvider};
use luminus_router::{
    AccountPool, ProviderAccount, ProviderRegistry, RouteCandidate, RoutePlan, Router as LRouter,
    RoutingPolicy,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
struct UpstreamState {
    expected_auth: String,
    status: StatusCode,
    requests: Arc<Mutex<usize>>,
    auth_ok: Arc<Mutex<bool>>,
}

struct LocalUpstream {
    base_url: String,
    state: UpstreamState,
    task: JoinHandle<()>,
}

impl LocalUpstream {
    async fn start(expected_auth: &str, status: StatusCode, body: serde_json::Value) -> Self {
        let state = UpstreamState {
            expected_auth: expected_auth.to_owned(),
            status,
            requests: Arc::new(Mutex::new(0)),
            auth_ok: Arc::new(Mutex::new(true)),
        };
        let response_body = Arc::new(body.to_string());
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    move |State(state): State<UpstreamState>, headers: HeaderMap| {
                        let response_body = response_body.clone();
                        async move {
                            *state.requests.lock().unwrap() += 1;
                            let actual = headers.get("authorization").and_then(|v| v.to_str().ok());
                            if actual != Some(state.expected_auth.as_str()) {
                                *state.auth_ok.lock().unwrap() = false;
                            }
                            (state.status, response_body.as_str().to_owned()).into_response()
                        }
                    },
                ),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            task,
        }
    }

    fn request_count(&self) -> usize {
        *self.state.requests.lock().unwrap()
    }
    fn auth_ok(&self) -> bool {
        *self.state.auth_ok.lock().unwrap()
    }
}

impl Drop for LocalUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn request() -> CanonicalRequest {
    CanonicalRequest {
        model: ModelId("bb/claude-sonnet-4.6".into()),
        messages: vec![CanonicalMessage::text(MessageRole::User, "hello")],
        tools: vec![],
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stop: None,
        stream: false,
        reasoning: None,
        metadata: None,
    }
}

fn success_body() -> serde_json::Value {
    json!({"id":"local-response","model":"bb/claude-sonnet-4.6","choices":[{"message":{"content":"local success"},"finish_reason":"stop"}]})
}

#[tokio::test]
async fn real_blackbox_provider_executes_against_localhost() {
    let upstream = LocalUpstream::start("Bearer test-single", StatusCode::OK, success_body()).await;
    let provider =
        BlackboxProvider::new(BlackboxConfig::new(&upstream.base_url, "test-single")).unwrap();
    let response = luminus_core::provider::ProviderAdapter::execute(
        &provider,
        &request(),
        &ProviderContext::new(
            "single",
            ProviderId::from("blackbox"),
            request().model.clone(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(upstream.request_count(), 1);
    assert!(upstream.auth_ok());
    assert_eq!(response.content, vec![ContentPart::text("local success")]);
    assert_eq!(response.model, request().model);
    assert_eq!(
        response.finish_reason,
        luminus_core::protocol::FinishReason::Stop
    );
}

#[tokio::test]
async fn provider_level_account_expansion_falls_back_between_real_blackbox_accounts() {
    let upstream_a = LocalUpstream::start(
        "Bearer test-account-a",
        StatusCode::TOO_MANY_REQUESTS,
        json!({"error":"rate limited"}),
    )
    .await;
    let upstream_b =
        LocalUpstream::start("Bearer test-account-b", StatusCode::OK, success_body()).await;
    let provider_a = Arc::new(
        BlackboxProvider::new(BlackboxConfig::new(&upstream_a.base_url, "test-account-a")).unwrap(),
    );
    let provider_b = Arc::new(
        BlackboxProvider::new(BlackboxConfig::new(&upstream_b.base_url, "test-account-b")).unwrap(),
    );
    let blackbox = ProviderId::from("blackbox");
    let model = request().model.clone();
    let mut pool = AccountPool::new();
    for (id, provider) in [
        ("test-blackbox-a", provider_a),
        ("test-blackbox-b", provider_b),
    ] {
        pool.register(ProviderAccount {
            descriptor: AccountDescriptor {
                id: AccountId::from(id),
                provider: blackbox.clone(),
                enabled: true,
            },
            adapter: provider,
        })
        .unwrap();
    }
    let router =
        LRouter::new(Arc::new(ProviderRegistry::new()), None).with_accounts(Arc::new(pool));
    let plan = RoutePlan {
        candidates: vec![RouteCandidate {
            provider: blackbox.clone(),
            model,
            account: None,
        }],
        policy: RoutingPolicy::new(2, true).unwrap(),
    };
    let execution = router
        .execute_plan(
            &request(),
            &plan,
            &ProviderContext::new("fallback", blackbox, plan.candidates[0].model.clone()),
        )
        .await
        .unwrap();
    assert_eq!(upstream_a.request_count(), 1);
    assert_eq!(upstream_b.request_count(), 1);
    assert!(upstream_a.auth_ok());
    assert!(upstream_b.auth_ok());
    assert_eq!(execution.attempts.len(), 2);
    assert_eq!(
        execution.attempts[0].account(),
        Some(&AccountId::from("test-blackbox-a"))
    );
    assert_eq!(
        execution.attempts[1].account(),
        Some(&AccountId::from("test-blackbox-b"))
    );
    assert_eq!(
        execution.response.content,
        vec![ContentPart::text("local success")]
    );
}

#[test]
fn credentials_are_not_exposed_by_account_descriptors() {
    let descriptor = AccountDescriptor {
        id: AccountId::from("test-blackbox-a"),
        provider: ProviderId::from("blackbox"),
        enabled: true,
    };
    let debug = format!("{descriptor:?}");
    assert!(!debug.contains("test-account-a"));
}
