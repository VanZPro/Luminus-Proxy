# Luminus-Proxy

Luminus-Proxy is an AI provider proxy that provides a unified OpenAI-compatible endpoint, account management, load balancing, and a web dashboard.

## Requirements

- [Bun](https://bun.sh) 1.x
- Python 3.10+
- Git

## Install

### Linux, macOS, or WSL

```bash
git clone <repository-url> luminus-proxy
cd luminus-proxy
bash install.sh
```

### Windows PowerShell

```powershell
git clone <repository-url> $HOME\luminus-proxy
cd $HOME\luminus-proxy
powershell -ExecutionPolicy Bypass -File install.ps1
```

The installer creates the local environment, installs dependencies, initializes configuration, and builds the dashboard.

## Start

```bash
luminus start
```

Open the dashboard at [http://localhost:1931](http://localhost:1931).

For development with hot reload:

```bash
luminus dev
```

## Useful commands

```bash
luminus status
luminus logs
luminus restart
luminus doctor
luminus update
```

## Configuration

Copy `.env.example` to `.env` if needed and configure the proxy API key, provider credentials, ports, and encryption key. Never commit secrets or production credentials.

## Rust development

Requirements:

- Rust stable
- Bun 1.x

The Rust backend is an incremental migration foundation and does not replace the existing TypeScript/Bun functionality yet.

Rust migration phases:

- R1: server foundation with Axum, Tokio, configuration, tracing, graceful shutdown, and `GET /health`.
- R2: protocol-neutral canonical AI domain model and provider abstraction.
- R3: translation-only adapters for OpenAI Chat Completions and Anthropic Messages.
- R4: an isolated Blackbox bearer-token transport proof of concept using reqwest.
- R5: Blackbox execution hardening with bounded response handling and safe error parsing.
- R6: experimental non-streaming OpenAI execution endpoint wired to the Blackbox adapter.
- R7: provider-neutral router and registry foundation for the experimental endpoint.
- R8: deterministic ordered fallback policy tested with fake providers; production still has only Blackbox.
- R9: provider/account separation and in-memory account-pool foundation.
- R10: deterministic account-level routing and fallback.
- R11: in-memory account health and cooldown eligibility.
- R12: deterministic account selection with FirstEligible and RoundRobin policies.
- R13: separate same-target retryability from cross-target fallbackability.
- R14: storage/persistence boundary and TypeScript database architecture audit.
- R15: isolated SQLite account metadata adapter validated only against temporary Rust-owned test databases.
- R16: read-only TypeScript account metadata compatibility adapter validated against synthetic fixtures.
- R17: credential/encryption architecture audit and typed secret-resolution boundary.
- R18: synthetic legacy encrypted-password compatibility reader and decoder.

R18 supports read-only compatibility with the verified TypeScript XOR/Base64 `accounts.password` format using an explicitly supplied synthetic key. It does not encrypt new Rust secrets with this legacy format, open a production database, read the production `ENCRYPTION_KEY`, parse tokens or metadata, or wire credentials into the server. Only synthetic fixtures are tested; the TypeScript backend remains production.

R15 uses an explicit database path and a separate SQLite adapter crate with a minimal Rust-native schema. R16 adds a separate read-only adapter for the TypeScript `accounts` metadata projection. R17 adds only architecture foundations: source-only credential/encryption audit, redacted `SecretString`, and a typed synthetic resolver contract. No production credentials were read, no production database was opened, no decryption implementation or server credential loading was added, and the existing Blackbox environment configuration remains active. The TypeScript/Bun backend remains production.

R14/R15 do not migrate the production database. The runtime `AccountPool` remains in-memory, secret persistence is deferred, and Blackbox is still the only real Rust provider.

```bash
cargo check --workspace
cargo test --workspace
cargo run -p luminus-server
```

The Rust server defaults to `127.0.0.1:1931`. If that port is occupied by the existing Bun backend, override it in PowerShell:

```powershell
$env:LUMINUS_PORT="1932"
cargo run -p luminus-server
```

Supported Rust configuration variables are `LUMINUS_HOST`, `LUMINUS_PORT`, `LUMINUS_ENV`, and `LUMINUS_LOG`.

### R2 architecture

Client protocol -> protocol adapter -> canonical request -> router -> provider adapter.
Responses follow the reverse path through the canonical response model. R2 defines these internal contracts only; it does not replace the existing TypeScript provider routing.

## License

MIT
