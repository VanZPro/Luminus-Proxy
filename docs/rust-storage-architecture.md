# R14 Rust Storage Boundary

## Scope

R14 is an architecture audit and persistence boundary foundation. It does not open, read, migrate, or write the existing production database. No Rust database driver or SQL is introduced.

## TypeScript persistence audit

The current backend uses Bun SQLite through `bun:sqlite` and Drizzle (`src/db/index.ts`). Initialization creates the configured parent directory, opens the configured database path, enables WAL mode, and enables foreign keys. The path comes from runtime configuration; this audit did not open the database or inspect secret values.

`src/db/schema.ts` defines these relevant areas:

- `accounts`: integer auto-increment ID, provider, email, encrypted password field, status, enabled flag, token JSON, quota counters/reset timestamps, usage/login timestamps, error text, provider-specific metadata JSON, and created/updated timestamps. `(provider, email)` is unique.
- `request_logs` and `usage_summary`: request and aggregate telemetry, including token/credit usage and account references. These are history/observability, not runtime routing state.
- `settings`: application-wide key/value settings with an updated timestamp. It is not account metadata.
- `model_mappings`, `api_keys`, `proxy_pool`, filters, image-studio data, and VCC tables: separate application concerns, not part of the Rust account repository.

Foreign keys are enabled at connection initialization. Account references in request/VCC transaction records have no explicit cascade action (SQLite default behavior); image results explicitly use `ON DELETE SET NULL`. The account uniqueness constraint is provider-scoped.

`src/db/migrate.ts` conditionally runs file migrations only when the Drizzle journal exists, then runs an ordered list of idempotent column-add operations on every invocation. The migration folder is documented as gitignored, so fresh deployments may skip file migrations and rely on the idempotent additions or `db:push`. This is a compatibility hazard for any future adapter and needs verification before reuse.

## Ownership classification

Generic persisted metadata is limited to account identity, provider identity, and enabled state in R14. Provider, email, status, timestamps, error text, and metadata remain legacy/provider/application fields until their domain meaning is verified.

Provider-specific configuration includes the existing account metadata JSON and provider-specific account behavior. No generic Rust configuration map is introduced.

Secrets include the account password and token JSON containing access/refresh authentication material, plus API keys and VCC data in their respective tables. The schema comments describe the password as encrypted, but the encryption/decryption implementation and key source require further verification; this audit did not print values or migrate the behavior. Decrypted credentials must be supplied only to provider-specific composition code.

Quota fields and Qoder free counters are persisted snapshots/counters tied to provider behavior and warmup synchronization. They are not ported to a generic Rust quota model. Request history, usage summaries, and last-used/login timestamps are persisted telemetry or derived state, not Router state.

`AccountHealthStore`, cooldown deadlines, last error categories, RoundRobin cursors, and request attempt history remain process-local runtime state and are not persisted.

## Rust boundary

The new `luminus-storage` crate depends only on `luminus-core` and `thiserror`. It does not depend on Router, Server, Axum, reqwest, provider implementations, TypeScript, or a database driver. Router has no storage dependency.

`StoredAccount` deliberately contains only `AccountId`, `ProviderId`, and `enabled`; it has no credential/configuration catch-all field. It converts to the existing `AccountDescriptor` without introducing runtime state. The types are separate because one is a persistence record and the other is a runtime descriptor, while sharing the same minimal semantic fields.

`AccountRepository` is a small read-oriented, object-safe async-compatible trait:

- `list_accounts()`
- `get_account(&AccountId)` returning `Result<Option<StoredAccount>, StorageError>`; missing records are represented by `None`.

The contract uses `Pin<Box<dyn Future<...> + Send + 'a>>` and does not add `async-trait`. `StorageError` is intentionally small and does not expose SQL errors, paths, or credentials.

## Memory repository and hydration

`MemoryAccountRepository` preserves input order, performs deterministic ID lookup, and rejects duplicate IDs with `InvalidRecord`. It represents persisted records and is not `AccountPool`: it has no adapters, health, cooldown, or selection cursor. Tests are fully offline and also verify use behind `Arc<dyn AccountRepository>`.

Future startup composition should load `StoredAccount` records, resolve provider-specific configuration and secrets through a separate provider/infrastructure boundary, construct `ProviderAccount` adapters, register them into the runtime `AccountPool`, and then give that pool to Router. Router must never query storage during request routing.

## Deferred work / recommended R15

Credential persistence and decryption APIs are deferred until the TypeScript secret boundary is verified. Provider configuration repositories, settings repositories, quota models, telemetry migration, schema compatibility handling, and SQLite adapters are also deferred. R15 should implement an isolated SQLite adapter against temporary test databases only, without reading or migrating the production TypeScript database.

The TypeScript/Bun backend and its Blackbox environment configuration remain production and unchanged.

## R14 safety result

No TypeScript files were modified. No database dependency, SQL, production database access, migration, provider migration, auth migration, streaming, production route, or routing semantic change was made.

Recommended next phase: R15 implement an isolated SQLite storage adapter against temporary test databases, without reading or migrating the production TypeScript database.
