use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
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
    app, build_experimental_snapshot, build_experimental_snapshot_with_legacy,
    experimental_diagnostics, legacy::ExperimentalLegacySourceConfig,
    parse_startup_config_with_parity, prepare_current_runtime,
};
use rusqlite::{Connection, params};
use tokio::net::TcpListener;
use tower::ServiceExt;

const NATIVE_KEY: &str = "SYNTHETIC_R26_NATIVE_KEY_DO_NOT_LEAK";
const LEGACY_KEY: &str = "SYNTHETIC_R26_LEGACY_KEY_DO_NOT_LEAK";
const LEGACY_API_KEY: &str = "SYNTHETIC_R26_LEGACY_API_KEY_DO_NOT_LEAK";
const TOKEN_SENTINEL: &str = "SYNTHETIC_R26_TOKEN_SECRET_DO_NOT_LEAK";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static UPSTREAM_REQUESTS: AtomicU64 = AtomicU64::new(0);

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
            "luminus-r26-server-{}-{}-{nonce}.db",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
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
                UPSTREAM_REQUESTS.fetch_add(1, Ordering::Relaxed);
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

#[tokio::test]
async fn native_parity_process_exits_without_upstream_http_or_bind() {
    let upstream_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = upstream_count.clone();
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = upstream_listener.accept().await else {
                break;
            };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            drop(socket);
        }
    });

    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let occupied_port = occupied.local_addr().unwrap().port().to_string();
    assert_eq!(upstream_count.load(std::sync::atomic::Ordering::SeqCst), 0);

    let binary = env::var("CARGO_BIN_EXE_luminus-server")
        .expect("Cargo must expose the luminus-server binary to integration tests");
    let mut child = Command::new(binary)
        .env("LUMINUS_HOST", "127.0.0.1")
        .env("LUMINUS_PORT", &occupied_port)
        .env("LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP", "true")
        .env("LUMINUS_EXPERIMENTAL_PARITY_DRY_RUN", "true")
        .env_remove("LUMINUS_EXPERIMENTAL_RUNTIME_DRY_RUN")
        .env_remove("LUMINUS_EXPERIMENTAL_LEGACY_SOURCE")
        .env_remove("LUMINUS_EXPERIMENTAL_LEGACY_DB_PATH")
        .env_remove("LUMINUS_EXPERIMENTAL_LEGACY_KEY")
        .env_remove("LUMINUS_EXPERIMENTAL_SOURCE_ORDER")
        .env("BLACKBOX_BASE_URL", format!("http://{upstream_address}"))
        .env("BLACKBOX_API_KEY", "SYNTHETIC_R31D_API_KEY_DO_NOT_LEAK")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn luminus-server binary");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "parity process exited unsuccessfully");
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("parity dry-run did not terminate");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(upstream_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(occupied.local_addr().is_ok());
}

#[test]
fn parity_legacy_conflict_rejects_before_filesystem_io() {
    let path = env::temp_dir().join(format!(
        "luminus-r31d-nonexistent-{}.db",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    assert!(!path.exists());
    let error = parse_startup_config_with_parity(
        Some("true"),
        Some("true"),
        Some(path.to_str().unwrap()),
        Some("SYNTHETIC_R31D_LEGACY_KEY_DO_NOT_LEAK"),
        Some("legacy-then-native"),
        None,
        Some("true"),
    )
    .expect_err("parity and legacy must conflict before I/O");
    assert!(error.to_string().contains("parity dry-run"));
    assert!(!path.exists());
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

async fn native_behavior_app(address: SocketAddr, key: &str) -> axum::Router {
    let (_, router) = prepare_current_runtime(format!("http://{address}"), key.to_owned())
        .expect("current native runtime prepares");
    app::experimental_app(Arc::new(router))
}

async fn experimental_behavior_app(address: SocketAddr, key: &str) -> axum::Router {
    let snapshot = build_experimental_snapshot(format!("http://{address}"), key.to_owned())
        .await
        .expect("experimental native runtime prepares");
    app::experimental_app(Arc::new(snapshot.router))
}

async fn response_json(app: axum::Router) -> (StatusCode, serde_json::Value) {
    let response = app.oneshot(request()).await.expect("request completes");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("response body reads");
    (
        status,
        serde_json::from_slice(&body).expect("response is JSON"),
    )
}

#[tokio::test]
async fn native_current_and_experimental_success_behavior_is_equivalent() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let state = seen.clone();
    let router = Router::new().route(
        "/{*path}",
        post(move |headers: HeaderMap| {
            let state = state.clone();
            async move {
                state.lock().unwrap().push(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()) == Some("Bearer SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK"),
                );
                (StatusCode::OK, r#"{"id":"r30a","model":"bb/claude-sonnet-4.6","choices":[{"message":{"content":"parity-success"},"finish_reason":"stop"}]}"#)
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let current = native_behavior_app(address, "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK").await;
    let experimental =
        experimental_behavior_app(address, "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK").await;
    let current_result = response_json(current).await;
    let experimental_result = response_json(experimental).await;
    assert_eq!(current_result.0, StatusCode::OK);
    assert_eq!(experimental_result.0, StatusCode::OK);
    assert_eq!(current_result.1, experimental_result.1);
    assert_eq!(seen.lock().unwrap().as_slice(), &[true, true]);
}

#[tokio::test]
async fn native_current_and_experimental_error_behavior_is_equivalent() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let state = seen.clone();
    let router = Router::new().route(
        "/{*path}",
        post(move |headers: HeaderMap| {
            let state = state.clone();
            async move {
                state.lock().unwrap().push(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        == Some("Bearer SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK"),
                );
                (StatusCode::TOO_MANY_REQUESTS, r#"{"error":"rate limited"}"#)
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let current = native_behavior_app(address, "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK").await;
    let experimental =
        experimental_behavior_app(address, "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK").await;
    let current_result = response_json(current).await;
    let experimental_result = response_json(experimental).await;
    assert_eq!(current_result, experimental_result);
    assert_eq!(current_result.0, StatusCode::BAD_GATEWAY);
    assert_eq!(seen.lock().unwrap().as_slice(), &[true, true]);
}

#[tokio::test]
async fn native_current_and_experimental_health_behavior_is_equivalent() {
    let current = native_behavior_app(
        "127.0.0.1:1".parse().unwrap(),
        "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK",
    )
    .await;
    let experimental = experimental_behavior_app(
        "127.0.0.1:1".parse().unwrap(),
        "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK",
    )
    .await;
    let current_response = current
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let experimental_response = experimental
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(current_response.status(), experimental_response.status());
    let current_body = axum::body::to_bytes(current_response.into_body(), 1024)
        .await
        .unwrap();
    let experimental_body = axum::body::to_bytes(experimental_response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(current_body, experimental_body);
}

#[tokio::test]
async fn readiness_is_intentionally_experimental_only() {
    let current = native_behavior_app(
        "127.0.0.1:1".parse().unwrap(),
        "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK",
    )
    .await;
    let snapshot = build_experimental_snapshot(
        "http://127.0.0.1:1".into(),
        "SYNTHETIC_R30A_API_KEY_DO_NOT_LEAK".into(),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, false));
    let experimental =
        app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics);
    let current_response = current
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let experimental_response = experimental
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(current_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(experimental_response.status(), StatusCode::OK);
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

#[tokio::test]
async fn native_only_readiness_is_safe_and_experimental_only() {
    let snapshot = build_experimental_snapshot("http://127.0.0.1:1".into(), NATIVE_KEY.into())
        .await
        .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, false));
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"ready\":true"));
    assert!(text.contains("\"legacy_enabled\":false"));
    assert!(!text.contains(NATIVE_KEY));
    assert!(!text.contains("blackbox-default"));
    let current = app::experimental_app(Arc::new(
        build_experimental_snapshot("http://127.0.0.1:1".into(), NATIVE_KEY.into())
            .await
            .unwrap()
            .router,
    ));
    let current_response = current
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(current_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn combined_readiness_survives_legacy_database_rename() {
    UPSTREAM_REQUESTS.store(0, Ordering::Relaxed);
    let native_address = upstream(
        "Bearer SYNTHETIC_R26_NATIVE_KEY_DO_NOT_LEAK",
        StatusCode::TOO_MANY_REQUESTS,
        Arc::new(Mutex::new(false)),
    )
    .await;
    assert_eq!(UPSTREAM_REQUESTS.load(Ordering::Relaxed), 0);
    let legacy_address = upstream(
        "Bearer SYNTHETIC_R26_LEGACY_API_KEY_DO_NOT_LEAK",
        StatusCode::OK,
        Arc::new(Mutex::new(false)),
    )
    .await;
    let fixture = Fixture::new(&format!("http://{legacy_address}"));
    let original = fixture.path.clone();
    let renamed = original.with_extension("ready-renamed.db");
    let snapshot = build_experimental_snapshot_with_legacy(
        format!("http://{native_address}/v1"),
        NATIVE_KEY.into(),
        legacy_config(&original, BlackboxSourceOrder::NativeThenLegacy),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, true));
    fs::rename(&original, &renamed).unwrap();
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(UPSTREAM_REQUESTS.load(Ordering::Relaxed), 0);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"legacy_enabled\":true"));
    assert!(text.contains("\"legacy_preflight\":\"passed\""));
    assert!(text.contains("\"source_order\":\"native-then-legacy\""));
    assert!(!text.contains("ready-renamed.db"));
    assert!(!text.contains(NATIVE_KEY));
    assert!(!text.contains(LEGACY_KEY));
    assert!(!text.contains(LEGACY_API_KEY));
    assert!(!text.contains(TOKEN_SENTINEL));
    drop(fixture);
    fs::remove_file(renamed).unwrap();
}

#[tokio::test]
async fn reverse_order_readiness_reports_configured_order_without_ids() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let snapshot = build_experimental_snapshot_with_legacy(
        "http://127.0.0.1:1/v1".into(),
        NATIVE_KEY.into(),
        legacy_config(&fixture.path, BlackboxSourceOrder::LegacyThenNative),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, true));
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"source_order\":\"legacy-then-native\""));
    assert!(!text.contains("blackbox-default"));
    assert!(!text.contains("legacy-ts:2301"));
}

#[tokio::test]
async fn empty_legacy_source_readiness_reports_zero_legacy_counts() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let connection = Connection::open(&fixture.path).unwrap();
    connection.execute("DELETE FROM accounts", []).unwrap();
    drop(connection);
    let snapshot = build_experimental_snapshot_with_legacy(
        "http://127.0.0.1:1/v1".into(),
        NATIVE_KEY.into(),
        legacy_config(&fixture.path, BlackboxSourceOrder::NativeThenLegacy),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, true));
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"ready\":true"));
    assert!(text.contains("\"legacy_preflight\":\"passed\""));
    assert!(text.contains("\"legacy_hydrated\":0"));
    assert!(text.contains("\"legacy_failed\":0"));
    assert!(text.contains("\"runtime_accounts\":1"));
}

#[tokio::test]
async fn readiness_aggregates_bad_and_good_legacy_accounts() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let connection = Connection::open(&fixture.path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET password = 'not-valid-ciphertext' WHERE id = 2301",
            [],
        )
        .unwrap();
    let tokens = r#"{"original_provider":"blackbox","base_url":"http://127.0.0.1:1","format":"openai","models":["bb/model"]}"#;
    connection.execute("INSERT INTO accounts (id, provider, email, password, enabled, tokens) VALUES (2302, 'byok', 'good@example.invalid', ?1, 1, ?2)", params![xor_base64(LEGACY_API_KEY, LEGACY_KEY), tokens]).unwrap();
    drop(connection);
    let snapshot = build_experimental_snapshot_with_legacy(
        "http://127.0.0.1:1/v1".into(),
        NATIVE_KEY.into(),
        legacy_config(&fixture.path, BlackboxSourceOrder::NativeThenLegacy),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, true));
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"legacy_hydrated\":1"));
    assert!(text.contains("\"legacy_failed\":1"));
    assert!(!text.contains("2301"));
    assert!(!text.contains("2302"));
}

#[tokio::test]
async fn readiness_aggregates_unsupported_legacy_accounts_as_skipped() {
    let fixture = Fixture::new("http://127.0.0.1:1");
    let connection = Connection::open(&fixture.path).unwrap();
    let tokens = r#"{"original_provider":"openai-compatible","base_url":"http://127.0.0.1:1","format":"openai","models":["model"]}"#;
    connection
        .execute("UPDATE accounts SET tokens = ?1", [tokens])
        .unwrap();
    drop(connection);
    let snapshot = build_experimental_snapshot_with_legacy(
        "http://127.0.0.1:1/v1".into(),
        NATIVE_KEY.into(),
        legacy_config(&fixture.path, BlackboxSourceOrder::NativeThenLegacy),
    )
    .await
    .unwrap();
    let diagnostics = Arc::new(experimental_diagnostics(&snapshot, true));
    let response = app::experimental_app_with_diagnostics(Arc::new(snapshot.router), diagnostics)
        .oneshot(
            Request::get("/experimental/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"legacy_hydrated\":0"));
    assert!(text.contains("\"legacy_skipped\":1"));
    assert!(!text.contains("2301"));
}
