# R17 Credential and Secret Architecture

## Observed TypeScript behavior

This audit was source-only. No production database, environment secret value, credential file, or ciphertext was opened.

The `accounts` table contains secret-bearing fields:

- `password`: required text field. Source writes encrypted passwords, API keys, PATs, and provider login passwords here.
- `tokens`: nullable JSON text containing provider-specific results such as access/refresh tokens, API keys, cookies, and other token material.
- `metadata`: nullable JSON text containing provider-specific data. The source also stores an encrypted Gmail password here for GitLab Duo.

`email` is account identity and is not itself treated as a secret by the audited schema. Quota, status, timestamps, and error fields are metadata/operational state.

Encryption is implemented in `src/utils/crypto.ts` using a repeating-key XOR over UTF-8 bytes followed by Base64 encoding. Decryption reverses the same operation. The source comment explicitly calls this XOR scheme insufficient for production and recommends AES-256-GCM, but no AES implementation was found.

The key is read from the `ENCRYPTION_KEY` environment variable through `src/config.ts`, with a hardcoded fallback string in source. No derivation, generation, external key store, version prefix, nonce, authentication tag, or ciphertext version marker is implemented. The exact historical format is therefore Base64 of XOR output using the configured UTF-8 key. Decryption has no authenticity check and the source does not validate decoded plaintext; malformed material can produce decoded text without a cryptographic failure. These details are observed source behavior, not a Rust implementation recommendation.

Encryption occurs in account-management and migration code, including `src/api/accounts.ts` and `src/api/migration-9router.ts`. Decryption occurs in account-management/provider execution paths in `src/api/accounts.ts`. The auth Python flow receives plaintext email/password as process inputs, obtains provider-specific token dictionaries, and returns them to TypeScript. The TypeScript runner persists returned token dictionaries into `accounts.tokens` and updates status/quota/metadata fields. Warmup can also persist refreshed `health.tokens` into `accounts.tokens`.

Provider credential shapes differ. Browser-login providers use email/password and may produce provider-specific token dictionaries and cookies. GitLab Duo uses a PAT in the encrypted `password` field and stores additional encrypted Gmail password material in metadata. BYOK and several API-key providers use the encrypted `password` field, and some legacy paths also duplicate API keys into `tokens`. Qoder and other providers use token JSON and provider metadata. The Python auth layer has provider-specific adapters and returns a generic token dictionary at that boundary. No single uniform credential shape is proven safe for Rust.

Credential ownership is split in the current system: database fields persist material, TypeScript account/auth orchestration chooses when to encrypt/decrypt and persist, and provider/Python adapters interpret the actual shape. This is legacy behavior; it is not being ported in R17.

Unresolved from source audit: historical ciphertext compatibility beyond the current helper, whether all existing rows use the current key, whether the fallback key has ever been used in deployed data, and complete provider-by-provider token schemas. No real decryption was performed.

## Proposed Rust architecture

R17 adds `crates/luminus-secrets`, depending only on `luminus-core` and `thiserror` at runtime. It does not depend on SQLite, providers, router, server, HTTP, or cryptography libraries.

`SecretString` owns a private `String`. Plaintext access requires the explicit `expose_secret()` method. It implements deliberate redacted `Debug` only. It intentionally does not implement `Display`, `Deref<str>`, `AsRef<str>`, `Serialize`, `Deserialize`, or `Clone`. No zeroization or crypto dependency is added in this architecture phase.

`CredentialRequest` contains only `AccountId` and `ProviderId`. `CredentialResolver<C>` returns a boxed future with typed provider-specific output and `SecretError`; it is object-safe for a fixed `C`, so it can be used behind `Arc<dyn CredentialResolver<C>>`. Errors are categorical and do not embed input, ciphertext, or secret values.

Tests use a test-only `SyntheticCredentials` type containing `SecretString` and an in-memory synthetic resolver. They prove explicit access, redacted debugging, account isolation, provider mismatch handling, not-found behavior, trait-object use, and secret-safe errors. No generic JSON credential bundle or string-keyed credential map exists.

## Boundaries and deferred work

`StoredAccount` remains credential-free. `LegacyTsAccountRepository` remains metadata-only and still selects only `id`, `provider`, and `enabled`. No secret SQLite repository, decryption implementation, production credential loading, server wiring, provider migration, router change, or AccountPool secret storage is added.

The intended future startup composition is:

`StoredAccount -> provider-specific resolver -> typed credentials -> provider composition -> ProviderAdapter -> ProviderAccount -> AccountPool -> Router`.

R17 defines only the safe contract and synthetic proof. R18 adds a separate `luminus-legacy-credentials` compatibility crate for the verified `accounts.password` format only. The TypeScript implementation uses TextEncoder UTF-8 bytes, repeating-key XOR indexed by byte position, standard Base64 encoding/decoding, and TextDecoder replacement semantics for invalid UTF-8. An empty key would make the TypeScript modulo operation invalid; Rust rejects it as `InvalidKey`. Malformed Base64 returns `InvalidCiphertext`, and invalid UTF-8 is rejected as `InvalidMaterial` because Rust cannot safely reproduce replacement semantics without silently changing credential material.

The R18 decoder accepts `LegacyCiphertext` and an explicitly supplied `SecretString` key. It performs no environment lookup and does not copy the TypeScript fallback-key value. `LegacyCiphertext` is privately stored and fully redacted in Debug. There is no public legacy encoder: XOR/Base64 is read compatibility only and must not be used for new Rust-native writes.

`LegacyPasswordReader` receives an explicit `PathBuf`, opens with `SQLITE_OPEN_READ_ONLY`, uses `spawn_blocking`, and selects only `id, provider, password FROM accounts ORDER BY id`. Lookups use bound parameters and reuse the R16 `legacy-ts:<numeric-id>` mapping. It does not select or parse email, tokens, metadata, quota, or other fields. Synthetic temporary fixtures cover fixed vectors, malformed Base64, empty keys, redaction, read-only SQL projection, deterministic lookup, and end-to-end decode to `SecretString`.

This XOR format has no nonce, authentication tag, integrity protection, or version marker. A wrong key can produce valid UTF-8, so successful decoding does not prove key correctness. Tokens, metadata, provider interpretation, production key policy, and production database access remain deferred.

## R19: typed provider-specific legacy resolution

### Observed TypeScript behavior

The source audit selected the `byok` mode as the simplest password-only interpretation. The 9router migration maps Blackbox and OpenAI-compatible connections to `byok`, and writes `data.apiKey` (or another imported token fallback) into encrypted `accounts.password`. The migration also writes provider-specific `tokens`, so R19 intentionally relies only on the password-only BYOK interpretation and does not claim to reproduce all imported connection semantics. Browser-login providers such as Kiro, Qoder, and GitLab Duo are deferred because their useful credentials depend on tokens, metadata, email/password, cookies, PAT distinctions, or refresh/session state.

### New Rust architecture

`crates/luminus-composition` owns `ByokCredentials { api_key: SecretString }` and `LegacyByokResolver`. The resolver implements `CredentialResolver<ByokCredentials>`, receives an explicit `LegacyPasswordReader` and `SecretString` legacy key, validates both the request provider and stored row provider, reads the encrypted password, decodes it, and returns the typed credential. Provider mismatches and decoder/storage failures map to safe `SecretError` categories without exposing values. The credential type has no raw public secret field and its Debug output remains redacted through `SecretString`.

R19 tests use temporary synthetic accounts databases and fake API-key values only. They cover typed interpretation, trait-object resolution, missing and foreign IDs, request/row provider mismatches, malformed ciphertext, empty explicit keys, redacted Debug, and isolation. No tokens or metadata are selected by the reader, and no resolver path performs HTTP, writes, server wiring, AccountPool hydration, or production access.

R19 stops at `ConcreteCredentials`. The next boundary is isolated synthetic startup hydration into `ProviderAccount` and `AccountPool`; it is not implemented here.

## R20: isolated startup hydration

`BlackboxAccountHydrator` is a composition-layer startup responsibility. It consumes `Arc<dyn AccountRepository>`, `Arc<dyn CredentialResolver<BlackboxCredentials>>`, and explicit typed-safe configuration containing only the Blackbox base URL. It loads safe metadata once, preserves repository order, filters to enabled `blackbox` records, resolves credentials only for eligible records, exposes `SecretString` only at the `BlackboxConfig` construction boundary, constructs the real `BlackboxProvider`, and registers `ProviderAccount` instances in `AccountPool`.

Disabled accounts are skipped without credential resolution. Unsupported providers are skipped for a future provider-specific hydrator. Individual credential failures become safe report categories and do not prevent later accounts from hydrating; repository failures remain fatal. Reports contain only account/provider IDs and typed categories, never credentials, ciphertext, paths, or raw lower-level errors. Account descriptors and pools remain secret-free, and no network request occurs during provider construction.

R20 uses synthetic `MemoryAccountRepository` records and an in-memory synthetic typed resolver. `LegacyByokResolver` is deliberately not connected: TypeScript `byok` is an umbrella mode that may represent Blackbox or OpenAI-compatible connections, so treating every legacy BYOK record as Blackbox would be unsafe. Provider-configuration resolution is deferred to R21. The server continues using its existing explicit `BLACKBOX_BASE_URL` and `BLACKBOX_API_KEY` path; no production database, environment lookup, AccountPool startup cutover, or router dependency change was made.
