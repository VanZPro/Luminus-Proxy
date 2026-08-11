# Rust Storage Architecture

## R16 read-only TypeScript account compatibility

R16 adds `LegacyTsAccountRepository` in `crates/luminus-storage-sqlite/src/legacy_ts.rs`. It implements the existing `AccountRepository` contract and requires an explicit database `PathBuf`; it does not discover, configure, or default to any production database path.

The current TypeScript source of truth is `src/db/schema.ts`: table `accounts`, integer auto-increment primary key `id`, required text `provider`, required text `email`, required text `password`, required text `status`, required integer `enabled` represented by Drizzle boolean mode as SQLite `0` or `1`, optional token JSON, quota/counter/timestamp fields, error text, metadata JSON, and timestamps. The `(provider, email)` unique index is provider-scoped. `src/db/migrate.ts` also adds newer account free-counter columns idempotently; historical deployments can therefore have migration drift. R16 requires the current selected metadata columns and does not support every historical state.

The adapter selects only:

```sql
SELECT id, provider, enabled FROM accounts ORDER BY id
```

`get_account` uses the same projection and a bound `WHERE id = ?1` parameter. There is no `SELECT *`. Password, email, tokens, quotas, timestamps, error text, and provider-specific metadata are never selected or parsed. Unknown extra columns do not affect the projection.

Legacy numeric IDs are encoded deterministically as `legacy-ts:<non-negative decimal id>`. This namespace avoids collision with native IDs, is independent of credentials, and is reversible for lookup. Non-legacy or malformed IDs return `Ok(None)`. Listing order is ascending legacy primary-key order only; it is not routing priority or account preference.

Each operation runs synchronous rusqlite work in `tokio::task::spawn_blocking`, opens a new connection with `SQLITE_OPEN_READ_ONLY`, executes the read, and closes the connection. The compatibility adapter contains no mutation SQL, migration, schema repair, or credential reader. Missing table/selected column and malformed selected values map to `StorageError::CorruptData`; missing records return `Ok(None)`; open failures remain generic and do not expose paths.

Tests create unique temporary synthetic SQLite fixtures, populate only fake sentinel values, and remove them through RAII cleanup. They cover current-shape listing and lookup, both enabled values, multiple providers including unsupported provider names, deterministic ordering, secret-column isolation, extra columns, missing table/column, malformed values, ID mapping, foreign IDs, `Arc<dyn AccountRepository>`, and fixture isolation. The production TypeScript database was not opened and no production data was read.

This compatibility claim means only that the safe account metadata projection can be read from a synthetic fixture modeled on the current TypeScript account schema. It does not provide credentials, token authentication, quotas, metadata, historical-schema compatibility, migration, provider hydration, `ProviderAccount`, `AccountPool`, startup wiring, or server integration. Credential encryption/key-source verification and the future provider-specific secret-resolution boundary remain deferred.

The existing Rust-native `SqliteAccountRepository` and its `luminus_accounts` schema remain separate and unchanged. `luminus-storage` remains database-independent, Router and Server have no database dependency, Blackbox remains environment-backed, and the TypeScript/Bun backend remains production.

## R15 isolated SQLite adapter

R15's Rust-owned adapter uses an explicit path, per-operation connections, `spawn_blocking`, bound parameters, and deterministic `ORDER BY id`. It is not TypeScript-schema compatible and was validated only against temporary Rust-owned fixtures.

## R14 boundary

`StoredAccount` contains only `AccountId`, `ProviderId`, and `enabled`. Storage is metadata persistence, not runtime provider execution or routing state. Credentials, settings, API keys, quotas, telemetry migration, and provider hydration remain separate deferred boundaries.
