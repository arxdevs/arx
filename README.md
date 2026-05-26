# arx

A self-hosted PaaS for home servers. Connect a GitHub repo, get an HTTPS service routed at a domain.

Written in Rust. Single Docker network, single GitHub App for identity + webhooks, single source of truth for routing (Traefik file provider). No web UI, no separate OAuth app, no bootstrap-token dance.

> **Status: early.** v1 is taking shape. Use at your own risk.

## Install

On the box that will run the daemon:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx setup
```

`install.sh` only places the `arx` CLI binary at `~/.local/bin/arx`. `arx setup` ensures the daemon (a `docker compose` stack of `arx` + `traefik`) is running, then walks you through creating a GitHub App and an admin domain.

On a client laptop:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx login --server https://arx.your-domain
```

## Quick start (server box)

```bash
arx setup
# - Daemon installed via docker compose
# - GitHub App created via manifest flow (browser-driven)
# - First user = the GitHub account that created the App
# - Default workspace "default" created
# - Optional: root domain + admin domain + ACME email

arx -w default project create --slug demo --name Demo

arx -w default -p demo service create \
    --slug pg --name Postgres --kind db --template postgres
arx -w default -p demo deploy pg

arx -w default -p demo service create \
    --slug web --name Web --kind git \
    --repo your-org/your-repo --branch main
arx -w default -p demo var set web DATABASE_URL='${{Postgres.DATABASE_URL}}'
arx -w default -p demo domain add web web.your-domain
arx -w default -p demo deploy web

# Push to the repo. The GitHub webhook triggers an auto-redeploy.
```

## What it does

- **Service types:** Git Source (Railpack or Dockerfile build), Docker Image, DB Template (postgres/mysql/mongodb/redis).
- **Routing:** Traefik in the same Docker network. ACME via HTTP-01.
- **Zero-downtime deploy:** TCP-or-HTTP healthcheck → Traefik route swap → old container removed.
- **Cross-service references:** `${{Postgres.DATABASE_URL}}` resolved at container start. Same `(workspace × project × environment)` scope.
- **Sealed variables:** write-only; injected into containers but never displayed.
- **DB backups:** scheduled local backups with retention; manual backup/restore.
- **Audit log:** 90-day retention, surfaces sensitive actions (sealed-variable changes, force-deletes, restores).

## Architecture in one paragraph

`docker-compose` brings up two containers: `arx` (the Rust daemon, JSON HTTP API on `127.0.0.1:7878`) and `traefik` (the reverse proxy, ports 80/443). They share one Docker network (`arx`) plus a named volume (`arx-data`). arx creates user service containers on the same network. Traefik routes by `Host` header to a stable per-service alias (a 12-char sha256 of `service_id || env_id`). arx is itself routed by Traefik at `arx.<root-domain>` so GitHub webhooks can reach it. The arx CLI talks to the daemon over HTTP using a Bearer token stored in `~/.arx/credentials.json`. For Git Source services, arx builds the user repo by either using an explicit `Dockerfile` in the repo, or — when none is present — auto-detecting one of the bundled stacks (Gradle/Maven JVM, Node, Python, Go) and piping a generated Dockerfile into `docker build`.

## Security model

**arx has access to `/var/run/docker.sock`.** That means the daemon can create privileged containers, mount any host path, and read any host file. Effectively, **arx admin = host root**.

Mitigations:

- arx binds only to `127.0.0.1:7878`. Public access happens via Traefik with TLS + GitHub OAuth.
- Variable values are encrypted at rest (ChaCha20-Poly1305). `master.key` lives only inside the `arx-data` volume.
- Sealed variables are write-only and never displayed.
- The audit log records sensitive actions for 90 days.

Out of scope for v1: socket proxies, rootless Docker, user-namespace remapping, container-runtime isolation (gVisor / kata).

**Do not run arx on a host where untrusted users can authenticate.**

**Network requirements:** `arx setup` verifies user-provided domains by querying `8.8.8.8` (UDP/53) directly rather than the OS resolver, so the daemon needs outbound UDP/53 to Google DNS. Split-horizon DNS environments where `8.8.8.8` is unreachable or returns different answers are unsupported in v1.

## Layout

```
arx/
├─ install.sh          # CLI installer (POSIX shell)
├─ compose.yml         # daemon stack: arx + traefik
├─ Dockerfile          # arx daemon image
├─ Cargo.toml          # workspace
├─ crates/
│  ├─ arx-core/        # domain types, refs parser, config, errors
│  ├─ arx-db/          # SQLite via sqlx; variable encryption
│  ├─ arx-traefik/     # dynamic.yml renderer + atomic writer
│  ├─ arx-docker/      # ContainerEngine trait + bollard impl
│  ├─ arx-build/       # Railpack / Dockerfile build orchestration
│  ├─ arx-github/      # webhook HMAC verify, GitHub API client
│  ├─ arx-server/      # axum HTTP API, deploy pipeline, schedulers
│  └─ arx-cli/         # clap CLI (arx binary)
└─ migrations/         # sqlx SQL files
```

## CLI cheatsheet

```
arx setup                                       # server box
arx login --server URL [--device]               # client box
arx logout
arx whoami

arx workspace list|create|delete
arx -w W project list|create|delete
arx -w W -p P service list|create|delete|show
arx -w W -p P -e E service rename <slug> <new-display-name>
arx -w W -p P service config set <slug> [--build-cmd "..."] [--start-cmd "..."]

arx -w W -p P -e E var list|set|unset|import
arx -w W -p P -e E domain add|remove|list
arx -w W -p P -e E config show|set

arx -w W -p P -e E deploy <service>
arx -w W -p P -e E rollback <service> <deployment-id>
arx -w W -p P -e E deployments <service>
arx -w W -p P -e E logs <service> [-f]

arx -w W -p P -e E backup now|list|restore|schedule

arx server install|upgrade|status
```

Universal flags: `--json` for machine output, `-q/--quiet` to suppress informational messages.

## License

MIT OR Apache-2.0
