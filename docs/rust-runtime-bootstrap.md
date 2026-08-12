# Rust Runtime Bootstrap

R24 adds an isolated `luminus-runtime-bootstrap` crate that coordinates the native Blackbox hydrator and the conclusively classified legacy BYOK Blackbox hydrator before constructing one Router.

`BlackboxRuntimeBootstrap` consumes already-constructed hydrators and an existing `ProviderRegistry`. It never reads environment variables, SQLite paths, credentials, or legacy encryption state. Hydrators can register directly into a caller-owned `AccountPool`; standalone `hydrate()` APIs remain available as compatibility wrappers.

`BlackboxSourceOrder` makes precedence explicit: `NativeThenLegacy` registers each source in its repository order, while `LegacyThenNative` reverses the source groups. Accounts are never globally sorted. AccountPool rejects duplicate runtime IDs, and source-level hydration errors prevent a successful snapshot. Individual account failures remain in the source reports and do not stop later accounts.

`RuntimeSnapshot` owns the same `Arc<AccountPool>` passed to Router. Router construction uses the existing constructor and fresh process-local health and selection state. No persisted health, cooldown, or RoundRobin cursor is used. After bootstrap, Router and AccountPool contain only normal runtime provider accounts; request execution does not resolve storage, configuration, credentials, or legacy state.

R24 remains synthetic and isolated. It does not open a production database, read production secrets, wire server startup, add a transport, or change the TypeScript production backend.

## R25 experimental server startup

R25 adds an explicit opt-in server path controlled by `LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP`. Missing, `false`, `off`, or `0` selects `Current`; `true`, `on`, or `1` selects `ExperimentalBootstrap`. Any other value fails safely during startup. The current environment-backed startup remains the default and preserves the `blackbox-default` account identity, `blackbox` provider, existing handlers, routing behavior, graceful shutdown, and `/health` contract.

The experimental path is native Blackbox-only. It creates safe in-memory `StoredAccount` metadata, resolves typed `BlackboxProviderConfig` and one-shot `BlackboxCredentials`, then runs `BlackboxAccountHydrator` through the native-only `BlackboxRuntimeBootstrap` path to produce the real `RuntimeSnapshot` and Router. `SecretString` remains non-Clone and no generic credential map is retained in server state. Request handlers are mode-agnostic and continue using `/experimental/v1/chat/completions`; no production `/v1` route or streaming path is enabled.

The path does not instantiate legacy resolvers, SQLite, `ENCRYPTION_KEY`, or any production database. It does not perform account writes or database cutover. A localhost synthetic Blackbox proof covers the typed native path and credential isolation. Rollback is to disable or remove the opt-in flag; no legacy server startup is enabled.
