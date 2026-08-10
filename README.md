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

## License

MIT
