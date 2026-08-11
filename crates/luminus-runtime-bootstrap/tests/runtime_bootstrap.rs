use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router as AxumRouter,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use base64::Engine;
use luminus_composition::{BlackboxAccountHydrator, BlackboxCredentials, BlackboxProviderConfig};
use luminus_core::{
    model::{AccountId, ModelId, ProviderId},
    protocol::{CanonicalMessage, CanonicalRequest, ContentPart, MessageRole},
    provider::ProviderContext,
};
use luminus_legacy_composition::LegacyByokBlackboxHydrator;
use luminus_legacy_credentials::LegacyPasswordReader;
use luminus_legacy_provider_config::LegacyByokConfigResolver;
use luminus_provider_config::{
    ProviderConfigRequest, ProviderConfigResolver, ProviderConfigResolverFuture,
};
use luminus_router::{ProviderRegistry, RouteCandidate, RoutePlan, RoutingPolicy};
use luminus_runtime_bootstrap::{
    BlackboxRuntimeBootstrap, BlackboxSourceOrder, RuntimeBootstrapError,
};
use luminus_secrets::{
    CredentialRequest, CredentialResolver, CredentialResolverFuture, SecretError, SecretString,
};
use luminus_storage::{
    AccountRepository, AccountRepositoryFuture, MemoryAccountRepository, StorageError,
    StoredAccount,
};
use luminus_storage_sqlite::LegacyTsAccountRepository;
use rusqlite::{Connection, params};
use tokio::{net::TcpListener, task::JoinHandle};

const NATIVE_KEY: &str = "SYNTHETIC_NATIVE_API_KEY_R24_DO_NOT_LEAK";
const LEGACY_KEY: &str = "SYNTHETIC_LEGACY_API_KEY_R24_DO_NOT_LEAK";
const XOR_KEY: &str = "SYNTHETIC_LEGACY_XOR_KEY_R24_DO_NOT_LEAK";
const TOKEN_SECRET: &str = "SYNTHETIC_TOKEN_SECRET_R24_DO_NOT_LEAK";
static DB_SERIAL: OnceLock<Mutex<u64>> = OnceLock::new();

struct Db {
    path: PathBuf,
}
impl Db {
    fn new(rows: &[(i64, &str, &str, &str)]) -> Self {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let serial_lock = DB_SERIAL.get_or_init(|| Mutex::new(0));
        let mut serial = serial_lock.lock().unwrap();
        *serial += 1;
        let path = std::env::temp_dir().join(format!("luminus-r24-{n}-{serial}.db"));
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE accounts (id INTEGER PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL, password TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active', enabled INTEGER NOT NULL DEFAULT 1, tokens TEXT NOT NULL, metadata TEXT, quota INTEGER, extra TEXT, UNIQUE(provider,email));").unwrap();
        for (id, email, password, tokens) in rows {
            c.execute("INSERT INTO accounts (id,provider,email,password,enabled,tokens) VALUES (?1,'byok',?2,?3,1,?4)", params![id,email,password,tokens]).unwrap();
        }
        Self { path }
    }
}
impl Drop for Db {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn encrypt(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(
        value
            .bytes()
            .enumerate()
            .map(|(i, b)| b ^ XOR_KEY.as_bytes()[i % XOR_KEY.len()])
            .collect::<Vec<_>>(),
    )
}
fn tokens(url: &str) -> String {
    format!(
        r#"{{"original_provider":"blackbox","base_url":"{url}","format":"openai","models":["blackbox-model"],"api_key":"{TOKEN_SECRET}"}}"#
    )
}

struct NativeCreds {
    values: std::collections::HashMap<AccountId, Result<String, SecretError>>,
    calls: Arc<Mutex<usize>>,
}
impl CredentialResolver<BlackboxCredentials> for NativeCreds {
    fn resolve<'a>(
        &'a self,
        r: &'a CredentialRequest,
    ) -> CredentialResolverFuture<'a, BlackboxCredentials> {
        *self.calls.lock().unwrap() += 1;
        let v = match self.values.get(&r.account_id) {
            Some(Ok(value)) => Ok(value.clone()),
            Some(Err(SecretError::InvalidMaterial)) => Err(SecretError::InvalidMaterial),
            Some(Err(SecretError::NotFound)) | None => Err(SecretError::NotFound),
            Some(Err(SecretError::Unavailable)) => Err(SecretError::Unavailable),
            Some(Err(SecretError::DecryptionFailed)) => Err(SecretError::DecryptionFailed),
            Some(Err(SecretError::Internal)) => Err(SecretError::Internal),
        };
        Box::pin(async move {
            v.map(|api_key| BlackboxCredentials {
                api_key: SecretString::new(api_key),
            })
        })
    }
}
struct NativeConfig {
    url: String,
    calls: Arc<Mutex<usize>>,
}
impl ProviderConfigResolver<BlackboxProviderConfig> for NativeConfig {
    fn resolve<'a>(
        &'a self,
        _: &'a ProviderConfigRequest,
    ) -> ProviderConfigResolverFuture<'a, BlackboxProviderConfig> {
        *self.calls.lock().unwrap() += 1;
        let url = self.url.clone();
        Box::pin(async move { Ok(BlackboxProviderConfig { base_url: url }) })
    }
}
fn native(
    ids: &[&str],
    url: &str,
    key: &str,
    failed: Option<&str>,
) -> (
    BlackboxAccountHydrator,
    Arc<Mutex<usize>>,
    Arc<Mutex<usize>>,
) {
    let records = ids
        .iter()
        .map(|id| StoredAccount::new(*id, "blackbox", true))
        .collect();
    let calls = Arc::new(Mutex::new(0));
    let config_calls = Arc::new(Mutex::new(0));
    let values = ids
        .iter()
        .map(|id| {
            (
                AccountId::from(*id),
                if Some(*id) == failed {
                    Err(SecretError::InvalidMaterial)
                } else {
                    Ok(key.to_owned())
                },
            )
        })
        .collect();
    let repo = Arc::new(MemoryAccountRepository::new(records).unwrap());
    (
        BlackboxAccountHydrator::new(
            repo,
            Arc::new(NativeCreds {
                values,
                calls: calls.clone(),
            }),
            Arc::new(NativeConfig {
                url: url.to_owned(),
                calls: config_calls.clone(),
            }),
        ),
        calls,
        config_calls,
    )
}
fn legacy(db: &Db) -> LegacyByokBlackboxHydrator {
    let repo = Arc::new(LegacyTsAccountRepository::new(&db.path));
    let config = Arc::new(LegacyByokConfigResolver::new(&db.path));
    let reader = Arc::new(LegacyPasswordReader::new(&db.path));
    let creds = Arc::new(luminus_composition::LegacyByokResolver::new(
        reader,
        SecretString::new(XOR_KEY),
    ));
    LegacyByokBlackboxHydrator::new(repo, config, creds)
}
fn bootstrap(
    native: BlackboxAccountHydrator,
    legacy: LegacyByokBlackboxHydrator,
    order: BlackboxSourceOrder,
) -> BlackboxRuntimeBootstrap {
    BlackboxRuntimeBootstrap::new(native, legacy, order, Arc::new(ProviderRegistry::new()))
}
fn ids(snapshot: &luminus_runtime_bootstrap::RuntimeSnapshot) -> Vec<AccountId> {
    snapshot
        .account_pool
        .ordered_ids_for_provider(&ProviderId::from("blackbox"))
}

#[tokio::test]
async fn combines_real_legacy_and_native_in_both_explicit_orders() {
    let db = Db::new(&[(
        2401,
        "l1",
        &encrypt(LEGACY_KEY),
        &tokens("http://legacy.invalid"),
    )]);
    let (n, _, _) = native(&["n1"], "http://native.invalid", NATIVE_KEY, None);
    let s = bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
        .build()
        .await
        .unwrap();
    assert_eq!(
        ids(&s),
        vec![AccountId::from("n1"), AccountId::from("legacy-ts:2401")]
    );
    assert!(
        ids(&s)
            .iter()
            .all(|id| s.account_pool.get(id).unwrap().descriptor.provider
                == ProviderId::from("blackbox"))
    );
    let (n, _, _) = native(&["n1"], "http://native.invalid", NATIVE_KEY, None);
    let s = bootstrap(n, legacy(&db), BlackboxSourceOrder::LegacyThenNative)
        .build()
        .await
        .unwrap();
    assert_eq!(
        ids(&s),
        vec![AccountId::from("legacy-ts:2401"), AccountId::from("n1")]
    );
}

#[tokio::test]
async fn preserves_internal_order_and_rejects_cross_source_duplicates() {
    let db = Db::new(&[
        (
            2402,
            "l1",
            &encrypt(LEGACY_KEY),
            &tokens("http://l1.invalid"),
        ),
        (
            2403,
            "l2",
            &encrypt(LEGACY_KEY),
            &tokens("http://l2.invalid"),
        ),
    ]);
    let (n, _, _) = native(&["n1", "n2"], "http://n.invalid", NATIVE_KEY, None);
    let s = bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
        .build()
        .await
        .unwrap();
    assert_eq!(
        ids(&s),
        vec![
            AccountId::from("n1"),
            AccountId::from("n2"),
            AccountId::from("legacy-ts:2402"),
            AccountId::from("legacy-ts:2403")
        ]
    );
    let db = Db::new(&[(2401, "l", &encrypt(LEGACY_KEY), &tokens("http://l.invalid"))]);
    let (n, _, _) = native(&["legacy-ts:2401"], "http://n.invalid", NATIVE_KEY, None);
    assert!(matches!(
        bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
            .build()
            .await,
        Err(RuntimeBootstrapError::DuplicateAccount)
    ));
}

#[tokio::test]
async fn source_failure_is_structural_and_account_failure_continues() {
    let db = Db::new(&[(2404, "l", &encrypt(LEGACY_KEY), &tokens("http://l.invalid"))]);
    let (n, nc, cc) = native(
        &["bad", "good"],
        "http://n.invalid",
        NATIVE_KEY,
        Some("bad"),
    );
    let s = bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
        .build()
        .await
        .unwrap();
    assert_eq!(
        ids(&s),
        vec![AccountId::from("good"), AccountId::from("legacy-ts:2404")]
    );
    assert_eq!(*nc.lock().unwrap(), 2);
    assert_eq!(*cc.lock().unwrap(), 2);
    struct Failing;
    impl AccountRepository for Failing {
        fn list_accounts(&self) -> AccountRepositoryFuture<'_, Vec<StoredAccount>> {
            Box::pin(async { Err(StorageError::Unavailable) })
        }
        fn get_account(&self, _: &AccountId) -> AccountRepositoryFuture<'_, Option<StoredAccount>> {
            Box::pin(async { Err(StorageError::Unavailable) })
        }
    }
    let bad = BlackboxAccountHydrator::new(
        Arc::new(Failing),
        Arc::new(NativeCreds {
            values: Default::default(),
            calls: Arc::new(Mutex::new(0)),
        }),
        Arc::new(NativeConfig {
            url: "http://x.invalid".into(),
            calls: Arc::new(Mutex::new(0)),
        }),
    );
    assert!(matches!(
        bootstrap(bad, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
            .build()
            .await,
        Err(RuntimeBootstrapError::SourceUnavailable(_))
    ));
}

#[tokio::test]
async fn reports_do_not_contain_synthetic_secrets_or_urls() {
    let db = Db::new(&[(
        2405,
        "l",
        &encrypt(LEGACY_KEY),
        &tokens("http://SYNTHETIC_BASE_URL_R24_DO_NOT_LEAK.invalid"),
    )]);
    let (n, _, _) = native(
        &["n"],
        "http://SYNTHETIC_NATIVE_URL_R24_DO_NOT_LEAK.invalid",
        NATIVE_KEY,
        Some("n"),
    );
    let s = bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
        .build()
        .await
        .unwrap();
    let text = format!("{:?}", s.report);
    for secret in [
        NATIVE_KEY,
        LEGACY_KEY,
        XOR_KEY,
        TOKEN_SECRET,
        "SYNTHETIC_NATIVE_URL_R24_DO_NOT_LEAK",
        "SYNTHETIC_BASE_URL_R24_DO_NOT_LEAK",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
}

#[derive(Clone)]
struct Upstream {
    expected: String,
    status: StatusCode,
    calls: Arc<Mutex<usize>>,
    auth_ok: Arc<Mutex<bool>>,
}
struct Server {
    base: String,
    state: Upstream,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(expected: &str, status: StatusCode, body: serde_json::Value) -> Server {
    let state = Upstream {
        expected: expected.into(),
        status,
        calls: Arc::new(Mutex::new(0)),
        auth_ok: Arc::new(Mutex::new(true)),
    };
    let body = Arc::new(body.to_string());
    let app = AxumRouter::new()
        .route(
            "/chat/completions",
            post(move |State(s): State<Upstream>, h: HeaderMap| {
                let body = body.clone();
                async move {
                    *s.calls.lock().unwrap() += 1;
                    if h.get("authorization").and_then(|x| x.to_str().ok())
                        != Some(s.expected.as_str())
                    {
                        *s.auth_ok.lock().unwrap() = false;
                    }
                    (s.status, body.as_str().to_owned()).into_response()
                }
            }),
        )
        .with_state(state.clone());
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server {
        base: format!("http://{addr}"),
        state,
        task,
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

#[tokio::test]
async fn real_router_uses_combined_snapshot_and_falls_back_with_isolated_credentials() {
    let n_up = server(
        "Bearer native-router-key",
        StatusCode::TOO_MANY_REQUESTS,
        serde_json::json!({"error":"rate limited"}),
    )
    .await;
    let l_up = server(&format!("Bearer {LEGACY_KEY}"), StatusCode::OK, serde_json::json!({"id":"ok","model":"bb/claude-sonnet-4.6","choices":[{"message":{"content":"success"},"finish_reason":"stop"}]})).await;
    let db = Db::new(&[(2406, "l", &encrypt(LEGACY_KEY), &tokens(&l_up.base))]);
    let (n, nc, cc) = native(&["n"], &n_up.base, "native-router-key", None);
    let snapshot = bootstrap(n, legacy(&db), BlackboxSourceOrder::NativeThenLegacy)
        .build()
        .await
        .unwrap();
    assert_eq!(
        ids(&snapshot),
        vec![AccountId::from("n"), AccountId::from("legacy-ts:2406")]
    );
    let before = (*nc.lock().unwrap(), *cc.lock().unwrap());
    let plan = RoutePlan {
        candidates: vec![RouteCandidate {
            provider: ProviderId::from("blackbox"),
            model: request().model.clone(),
            account: None,
        }],
        policy: RoutingPolicy::new(2, true).unwrap(),
    };
    let result = snapshot
        .router
        .execute_plan(
            &request(),
            &plan,
            &ProviderContext::new("r24", ProviderId::from("blackbox"), request().model),
        )
        .await
        .unwrap();
    assert_eq!(result.attempts.len(), 2);
    assert_eq!(*n_up.state.calls.lock().unwrap(), 1);
    assert_eq!(*l_up.state.calls.lock().unwrap(), 1);
    assert!(*n_up.state.auth_ok.lock().unwrap());
    assert!(*l_up.state.auth_ok.lock().unwrap());
    assert_eq!(*nc.lock().unwrap(), before.0);
    assert_eq!(*cc.lock().unwrap(), before.1);
    assert_eq!(result.response.content, vec![ContentPart::text("success")]);
}
