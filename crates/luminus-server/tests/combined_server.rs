use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    response::IntoResponse,
    routing::post,
};
use base64::Engine;
use luminus_core::model::{AccountId, ProviderId};
use luminus_runtime_bootstrap::BlackboxSourceOrder;
use luminus_secrets::SecretString;
use luminus_server::{
    app, build_experimental_snapshot_with_legacy, legacy::ExperimentalLegacySourceConfig,
};
use rusqlite::{Connection, params};
use tokio::net::TcpListener;
use tower::ServiceExt;

const NATIVE_KEY: &str = "SYNTHETIC_R26_NATIVE_KEY_DO_NOT_LEAK";
const LEGACY_KEY: &str = "SYNTHETIC_R26_LEGACY_KEY_DO_NOT_LEAK";
const LEGACY_API_KEY: &str = "SYNTHETIC_R26_LEGACY_API_KEY_DO_NOT_LEAK";
const TOKEN_SENTINEL: &str = "SYNTHETIC_R26_TOKEN_SECRET_DO_NOT_LEAK";

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn new(legacy_base_url: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "luminus-r26-server-{}-{nonce}.db",
            std::process::id()
        ));
        let connection = Connection::open(&path).expect("create synthetic SQLite fixture");
        connection
            .execute_batch(
                "CREATE TABLE accounts (
                    id INTEGER PRIMARY KEY,
                    provider TEXT NOT NULL,
                    email TEXT,
                    password TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    tokens TEXT NOT NULL
                );",
            )
            .expect("create synthetic accounts table");
        let encrypted = xor_base64(LEGACY_API_KEY, LEGACY_KEY);
        let tokens = format!(
            r#"{{"original_provider":"blackbox","base_url":"{legacy_base_url}","format":"openai","models":["bb/claude-sonnet-4.6"],"api_key":"{TOKEN_SENTINEL}","token":"{TOKEN_SENTINEL}"}}"#
        );
        connection
            .execute(
                "INSERT INTO accounts (id, provider, email, password, enabled, tokens) VALUES (?1, 'byok', ?2, ?3, 1, ?4)",
                params![2301_i64, "synthetic@example.invalid", encrypted, tokens],
            )
            .expect("insert synthetic legacy account");
        drop(connection);
        Self { path }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn xor_base64(value: &str, key: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(
        value
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ key.as_bytes()[index % key.len()])
            .collect::<Vec<_>>(),
    )
}

async fn upstream(
    expected_auth: &'static str,
    status: StatusCode,
    seen: Arc<Mutex<bool>>,
) -> SocketAddr {
    let router = Router::new().route(
        "/{*path}",
        post(move |headers: HeaderMap| {
            let seen = seen.clone();
            async move {
                *seen.lock().expect("upstream state lock") = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    == Some(expected_auth);
                if status == StatusCode::OK {
                    (status, r#"{"id":"r26","model":"bb/claude-sonnet-4.6","choices":[{"message":{"content":"legacy-success"},"finish_reason":"stop"}]}"#).into_response()
                } else {
                    (status, "rate limited").into_response()
                }
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind synthetic upstream");
    let address = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve synthetic upstream")
    });
    address
}

fn legacy_config(path: &Path, order: BlackboxSourceOrder) -> ExperimentalLegacySourceConfig {
    ExperimentalLegacySourceConfig {
        database_path: path.to_path_buf(),
        legacy_key: SecretString::new(LEGACY_KEY),
        source_order: order,
    }
}

fn request() -> Request<Body> {
    Request::post("/experimental/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"bb/claude-sonnet-4.6","messages":[{"role":"User","content":"hello"}]}"#,
        ))
        .expect("build chat request")
}

#[tokio::test]
async fn combined_server_fallback_survives_legacy_database_rename() {
    let native_seen = Arc::new(Mutex::new(false));
    let legacy_seen = Arc::new(Mutex::new(false));
    let native_address = upstream(
        "Bearer SYNTHETIC_R26_NATIVE_KEY_DO_NOT_LEAK",
        StatusCode::TOO_MANY_REQUESTS,
        native_seen.clone(),
    )
    .await;
    let legacy_address = upstream(
        "Bearer SYNTHETIC_R26_LEGACY_API_KEY_DO_NOT_LEAK",
        StatusCode::OK,
        legacy_seen.clone(),
    )
    .await;
    let fixture = Fixture::new(&format!("http://{legacy_address}"));
    let original = fixture.path.clone();
    let renamed = original.with_extension("renamed.db");
    let snapshot = build_experimental_snapshot_with_legacy(
        format!("http://{native_address}/v1"),
        NATIVE_KEY.into(),
        legacy_config(&original, BlackboxSourceOrder::NativeThenLegacy),
    )
    .await
    .expect("combined R26 startup must succeed");
    assert_eq!(
        snapshot
            .account_pool
            .ordered_ids_for_provider(&ProviderId::from("blackbox")),
        vec![
            AccountId::from("blackbox-default"),
            AccountId::from("legacy-ts:2301")
        ]
    );
    fs::rename(&original, &renamed)
        .expect("startup must release all SQLite handles before serving");
    let response = app::experimental_app(Arc::new(snapshot.router))
        .oneshot(request())
        .await
        .expect("server request must complete after database rename");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(
        *native_seen.lock().unwrap(),
        "native upstream did not receive its own credential"
    );
    assert!(
        *legacy_seen.lock().unwrap(),
        "legacy upstream did not receive its own credential"
    );
    assert!(!String::from_utf8_lossy(&body).contains(LEGACY_KEY));
    // The renamed source proves request handling cannot reopen SQLite or invoke legacy resolvers.
    drop(fixture);
    fs::remove_file(renamed).expect("remove renamed synthetic fixture");
}

#[tokio::test]
async fn combined_server_legacy_then_native_preserves_startup_order() {
    let legacy_address = upstream(
        "Bearer SYNTHETIC_R26_LEGACY_API_KEY_DO_NOT_LEAK",
        StatusCode::OK,
        Arc::new(Mutex::new(false)),
    )
    .await;
    let fixture = Fixture::new(&format!("http://{legacy_address}"));
    let snapshot = build_experimental_snapshot_with_legacy(
        "http://127.0.0.1:1/v1".into(),
        NATIVE_KEY.into(),
        legacy_config(&fixture.path, BlackboxSourceOrder::LegacyThenNative),
    )
    .await
    .expect("reverse-order R26 startup must succeed");
    assert_eq!(
        snapshot
            .account_pool
            .ordered_ids_for_provider(&ProviderId::from("blackbox")),
        vec![
            AccountId::from("legacy-ts:2301"),
            AccountId::from("blackbox-default")
        ]
    );
}
