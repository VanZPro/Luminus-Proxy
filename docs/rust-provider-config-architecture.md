# R21 Provider Configuration Architecture

Provider configuration is separate from credentials. `ProviderConfigRequest` carries only safe account/provider identity; `ProviderConfigResolver<C>` returns a provider-specific typed configuration behind an object-safe fixed-`C` trait object. Configuration errors are categorical and never include secrets or raw configuration.

`luminus-provider-config` depends only on `luminus-core` and `thiserror`. The concrete `BlackboxProviderConfig { base_url }` lives in `luminus-composition` and contains no API key. Hydration resolves configuration before credentials, skips disabled/unrelated records, reports individual configuration failures safely, continues in repository order, and constructs the real provider only after both typed values exist. Construction remains network-free.

## Legacy BYOK source audit

Audited `src/api/migration-9router.ts`, `src/api/accounts.ts`, and BYOK runtime references in `src/proxy/providers`. The migration maps exact source providers `blackbox` and providers whose name starts with `openai-compatible` to Luminus `provider = "byok"`. Blackbox is distinguished before migration by the exact source provider value `blackbox`; OpenAI-compatible is distinguished by the `openai-compatible` prefix.

The migration writes both into the same `accounts.provider` value. It preserves `original_provider` and other fields inside `accounts.tokens`, but those blobs also contain `api_key`; no dedicated persisted subtype column exists. OpenAI-compatible configuration uses `providerSpecificData.baseUrl` or `data.baseUrl`, normalized only by current native BYOK update paths (trailing slash removal), while Blackbox migration supplies a hard-coded URL and model list. Native BYOK stores `base_url`, `format`, and `models` in `tokens`, alongside API-key material. Models are not a safe subtype discriminator, and no contractual URL heuristic is used.

Therefore the audit classification is **NOT CONCLUSIVE** for a generic persisted BYOK row. Legacy BYOK subtype cannot be resolved conclusively from the audited persisted fields without interpreting a secret-bearing mixed blob, and historical rows may lack the migration markers. No tokens or metadata compatibility parser was added. `LegacyByokResolver` remains credential-only and disconnected from Blackbox hydration. Safe mapping requires a future isolated adapter only where a typed, conclusively preserved discriminator is available; ambiguous rows remain unresolved.

Production database, credentials, and environment values were not opened or read. Server startup remains environment-backed and unchanged.