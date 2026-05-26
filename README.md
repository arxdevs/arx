# arx

**arx is a deploy agent for your home server.** Point it at a GitHub repo; it builds, routes through Traefik with ACME TLS, and keeps the service alive across zero-downtime swaps.

> Status: early. v1 is taking shape. Use at your own risk.

## Install

Two paths — pick whichever fits.

**Run it yourself.** On the box that will run the daemon:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx setup
```

`arx setup` walks you through creating a GitHub App, your admin domain, and the daemon stack. For client laptops add `arx login --server https://arx.<root-domain>`.

**Have an AI agent do it.** Paste this into Claude Code, Codex, Cursor, or another coding agent:

> Read https://raw.githubusercontent.com/arxdevs/arx/main/install.md and install arx for me. Ask me whatever you need.

The agent then follows [install.md](./install.md), asks you whether to install the server, the client, or both, prompts you for the root domain / ACME email at the right moments, and verifies the daemon at the end.

## Deploy a repo

```bash
arx -w default project create --slug demo --name Demo
arx -w default -p demo service create \
    --slug web --name Web --kind git \
    --repo your-org/your-repo --branch main
arx -w default -p demo domain add web web.your-domain
arx -w default -p demo deploy web
```

For Git Source services arx auto-detects the build stack (Gradle/Maven JVM, Node, Python, Go) and renders an in-memory Dockerfile. To override the inferred commands:

```bash
arx -w default -p demo service config set web \
    --build-cmd "..." --start-cmd "..."
```

If your repo has a `Dockerfile` at its root, arx uses it as-is.

## What it does

- **Service types:** Git Source (auto-detected stack or `Dockerfile`), Docker Image, DB Template (postgres/mysql/mongodb/redis).
- **Routing:** Traefik in the same Docker network. ACME via HTTP-01.
- **Zero-downtime deploy:** TCP-or-HTTP healthcheck → Traefik route swap → old container removed.
- **Cross-service references:** `${{Postgres.DATABASE_URL}}` resolved at container start, scoped to `(workspace × project × environment)`.
- **Sealed variables:** write-only; injected into containers but never displayed.
- **DB backups:** scheduled local backups with retention; manual restore.
- **Audit log:** 90-day retention for sensitive actions (sealed-variable changes, force-deletes, restores).

## Architecture

`docker compose` brings up two containers — `arx` (Rust daemon, JSON HTTP API on `127.0.0.1:7878`) and `traefik` (ports 80/443) — sharing one Docker network and one named volume. arx creates user service containers on the same network. Traefik routes by `Host` header to a stable per-service alias (12-char sha256 of `service_id || env_id`). arx is itself routed by Traefik at `arx.<root-domain>` so GitHub webhooks can reach it.

For Git Source builds, arx detects the stack from manifests in the repo (`build.gradle*`, `pom.xml`, `package.json`, `pyproject.toml` or `requirements.txt`, `go.mod`), renders a Dockerfile in memory, and pipes it into `docker build -f -`. No template file is written into the user's repo.

## Security model

**arx has access to `/var/run/docker.sock`.** It can create privileged containers, mount any host path, and read any host file. Effectively, **arx admin = host root**.

Mitigations:

- arx binds only to `127.0.0.1:7878`. Public access happens via Traefik with TLS + GitHub OAuth.
- Variable values are encrypted at rest with ChaCha20-Poly1305. The master key lives only inside the `arx-data` Docker volume.
- Sealed variables are write-only and never displayed.
- Audit log records sensitive actions for 90 days.

Out of scope for v1: socket proxies, rootless Docker, user-namespace remapping, container-runtime isolation (gVisor / kata).

**Network requirement:** `arx setup` verifies user-provided domains by querying `8.8.8.8` directly rather than the OS resolver. Split-horizon DNS environments where `8.8.8.8` is unreachable are unsupported in v1.

**Do not run arx on a host where untrusted users can authenticate.**

## CLI

```
arx --help
arx <noun> --help
```

Every command takes `--json` for machine-readable output and `-q/--quiet` to suppress informational messages.

## License

Dual-licensed under [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE), at your option.
