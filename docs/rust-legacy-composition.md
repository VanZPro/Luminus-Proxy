# Rust R23 Legacy Composition

R23 is an isolated startup-time compatibility bridge for synthetic legacy BYOK data.

## Classification-gated transition

Legacy storage uses `provider = byok` as a persistence umbrella. It is not a runtime provider identity. The bridge requests configuration with provider `byok`, then accepts only the typed `LegacyByokConfig::Blackbox` variant. Only after that conclusive classification does the runtime descriptor use provider `blackbox`.

The `AccountId` remains exactly `legacy-ts:<id>`. SQLite is never rewritten.

OpenAI-compatible variants, missing or unknown discriminators, malformed configuration, disabled rows, and unrelated providers are skipped. No credential resolver call occurs for those cases.

## Credential and configuration boundaries

Legacy credentials are requested as `byok` and are represented by `ByokCredentials`. Inside the already-matched Blackbox branch, the `SecretString` is moved into `BlackboxCredentials`. There is no global `From<ByokCredentials>` conversion. Plaintext access occurs only at the concrete Blackbox provider construction boundary.

The safe legacy Blackbox base URL is mapped to `BlackboxProviderConfig`. Legacy format and model-list fields remain typed compatibility data but are not forced into the current Blackbox transport, router, or account descriptor.

## Runtime behavior

The reusable Blackbox construction helper creates the real `BlackboxProvider`, a normal `ProviderAccount`, and a runtime descriptor with `provider = blackbox`. The legacy composition crate performs no SQLite access itself and keeps legacy resolvers out of `AccountPool` and the request path.

Hydration preserves repository order and reports only categorical, account-scoped outcomes. Reports never contain raw JSON, ciphertext, API keys, URLs, paths, or lower-level error text.

## Validation and scope

Tests use synthetic resolvers and temporary databases only. Production database rows, production credentials, environment keys, server startup, router semantics, and TypeScript runtime behavior are untouched. There is no OpenAI-compatible transport and no server cutover in R23.

The current R23 tests verify classification-before-credential access, runtime provider rewriting, ordering, and disabled/unrelated account filtering. A real SQLite end-to-end and localhost Router test remain future hardening work; they are not enabled by production wiring.
