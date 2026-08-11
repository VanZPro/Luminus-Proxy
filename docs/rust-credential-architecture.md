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

R17 defines only the safe contract and synthetic proof. Compatible legacy credential-row reading and cryptographic compatibility are deferred until a later phase confirms the required format and key boundary without accessing production data.
