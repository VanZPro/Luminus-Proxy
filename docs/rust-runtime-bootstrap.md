# Rust Runtime Bootstrap

R24 adds an isolated `luminus-runtime-bootstrap` crate that coordinates the native Blackbox hydrator and the conclusively classified legacy BYOK Blackbox hydrator before constructing one Router.

`BlackboxRuntimeBootstrap` consumes already-constructed hydrators and an existing `ProviderRegistry`. It never reads environment variables, SQLite paths, credentials, or legacy encryption state. Hydrators can register directly into a caller-owned `AccountPool`; standalone `hydrate()` APIs remain available as compatibility wrappers.

`BlackboxSourceOrder` makes precedence explicit: `NativeThenLegacy` registers each source in its repository order, while `LegacyThenNative` reverses the source groups. Accounts are never globally sorted. AccountPool rejects duplicate runtime IDs, and source-level hydration errors prevent a successful snapshot. Individual account failures remain in the source reports and do not stop later accounts.

`RuntimeSnapshot` owns the same `Arc<AccountPool>` passed to Router. Router construction uses the existing constructor and fresh process-local health and selection state. No persisted health, cooldown, or RoundRobin cursor is used. After bootstrap, Router and AccountPool contain only normal runtime provider accounts; request execution does not resolve storage, configuration, credentials, or legacy state.

R24 remains synthetic and isolated. It does not open a production database, read production secrets, wire server startup, add a transport, or change the TypeScript production backend.

## R25 experimental server startup

R25 adds an explicit opt-in server path controlled by `LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP`. Missing, `false`, `off`, or `0` selects `Current`; `true`, `on`, or `1` selects `ExperimentalBootstrap`. Any other value fails safely during startup. The current environment-backed startup remains the default and preserves the `blackbox-default` account identity, `blackbox` provider, existing handlers, routing behavior, graceful shutdown, and `/health` contract.

The experimental path is native Blackbox-only. It creates safe in-memory `StoredAccount` metadata, resolves typed `BlackboxProviderConfig` and one-shot `BlackboxCredentials`, then runs `BlackboxAccountHydrator` through the native-only `BlackboxRuntimeBootstrap` path to produce the real `RuntimeSnapshot` and Router. `SecretString` remains non-Clone and no generic credential map is retained in server state. Request handlers are mode-agnostic and continue using `/experimental/v1/chat/completions`; no production `/v1` route or streaming path is enabled.

The path does not instantiate legacy resolvers, SQLite, `ENCRYPTION_KEY`, or any production database. It does not perform account writes or database cutover. Rollback is to disable or remove the opt-in flag. The TypeScript/Bun backend remains the production path.

R26C adds real server-level synthetic integration coverage: combined native/legacy startup, `NativeThenLegacy` fallback, `LegacyThenNative` ordering, credential and base-URL isolation, and successful request handling after the synthetic SQLite file is renamed. This demonstrates that the legacy repository, resolvers, decryption key, and database path are startup-only and are not needed by the request path.

## R26 explicitly configured legacy source

R26 keeps the R25 bootstrap opt-in and adds a second, independent opt-in: `LUMINUS_EXPERIMENTAL_LEGACY_SOURCE`. Accepted enabled values are `true`, `on`, and `1`; disabled values are absent, `false`, `off`, and `0`. Any malformed explicit value is a safe startup configuration error.

Legacy access requires both `LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP=true` (or `on`/`1`) and `LUMINUS_EXPERIMENTAL_LEGACY_SOURCE=true` (or `on`/`1`). Configuration is parsed and cross-validated before startup dispatch. Enabling legacy under `Current` fails before SQLite access and does not continue through current startup. Legacy-specific values supplied while legacy is disabled are also rejected as contradictory configuration; they never activate legacy implicitly.

When enabled, all of these must be explicit:

- `LUMINUS_EXPERIMENTAL_LEGACY_DB_PATH`: the database file path. There is no default, process-working-directory lookup, discovery, copy, or backup.
- `LUMINUS_EXPERIMENTAL_LEGACY_KEY`: exactly one non-empty caller-supplied legacy decryption key. It is held as `SecretString`; neither `ENCRYPTION_KEY` nor the historical TypeScript fallback key is consulted.
- `LUMINUS_EXPERIMENTAL_SOURCE_ORDER`: either `native-then-legacy` or `legacy-then-native`.

Configuration parsing performs no SQLite access. After parsing succeeds, a narrow structural preflight checks path existence, rejects directories, opens the database with `SQLITE_OPEN_READ_ONLY`, verifies the `accounts` table, and verifies the compatibility columns `id`, `provider`, `enabled`, `password`, and `tokens`. Preflight performs no `CREATE`, `ALTER`, `INSERT`, `UPDATE`, `DELETE`, `DROP`, `VACUUM`, migration, write-affecting PRAGMA, credential decryption, or token JSON scan. A structurally valid empty table is accepted.

Structural failures are fail-closed: missing or invalid path, read-only open failure, invalid SQLite, missing table/column, empty key, invalid order, and source failure abort startup. There is no native-only fallback after legacy was explicitly requested and failed. Safe error categories do not expose raw SQLite errors, SQL, full private paths, keys, passwords, ciphertext, tokens, emails, or base URLs.

After preflight, the server composes the existing `LegacyTsAccountRepository`, `LegacyByokConfigResolver`, `LegacyPasswordReader`, `LegacyByokResolver`, and `LegacyByokBlackboxHydrator` with the native Blackbox hydrator through `BlackboxRuntimeBootstrap`, using the explicit source order to construct one `RuntimeSnapshot` and the existing Router. Runtime identity maps conclusively classified `byok`/`blackbox` rows to the Blackbox runtime only; SQLite is not retained in Router request state.

Per-account failures remain non-fatal after structural preflight. Ambiguous BYOK rows without `original_provider` are unresolved and are not hydrated. OpenAI-compatible rows remain unsupported for the Blackbox runtime and are not transported. A malformed credential or configuration for one account is reported safely while later valid accounts continue. No OpenAI-compatible transport or other provider is added.

The experimental endpoint remains `/experimental/v1/chat/completions`; no production route, streaming, CRUD, migration, writeback, or Rust database cutover is introduced. Health behavior is unchanged and does not expose legacy details. To roll back R26, unset or disable the legacy flag and, if desired, disable the R25 bootstrap flag; the TypeScript backend remains production.

All development fixtures use temporary synthetic SQLite files only. No production database, `.env`, user account, password, token, or historical fallback key is inspected.

## R27 safe experimental readiness

R27 adds `GET /experimental/ready` only to the experimental bootstrap application. The existing `GET /health` liveness contract is unchanged, and the default/current application does not register the readiness route. A successful experimental startup creates an immutable safe aggregate from the already-computed bootstrap report: runtime mode, runtime account count, native hydrated/failed counts, legacy enabled/preflight/hydrated/failed/skipped counts, and explicit source order.

Readiness describes the successful startup snapshot currently being served. It performs no provider HTTP probe, SQLite access, repository access, provider-config resolution, credential resolution, polling, reload, or hot-watch. The diagnostics response never enumerates account IDs and contains no database path, base URL, email, API key, legacy decryption key, ciphertext, token JSON, or raw lower-level errors. Structural legacy failures still abort startup under R26; readiness is not a deferred 503 state. The TypeScript/Bun backend remains production and default startup behavior is unchanged.
