# arx

**arx is a deploy agent for your home server.** Point it at a GitHub repo; it builds, routes through Traefik with ACME TLS, and keeps the service alive across zero-downtime swaps.

> Status: early. v1 is taking shape. Use at your own risk.

## Install

Run it yourself on the box that will host the daemon:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx setup
```

Or have a coding agent (Claude Code, Codex, Cursor, Copilot) do it:

```
Read https://raw.githubusercontent.com/arxdevs/arx/main/install.md and install arx
```

## Deploy a repo

```bash
arx -w default project create --slug demo --name Demo
arx -w default -p demo service create \
    --slug web --name Web --kind git \
    --repo your-org/your-repo --branch main
arx -w default -p demo domain add web web.your-domain
arx -w default -p demo deploy web
```

arx auto-detects the build stack (Gradle/Maven JVM, Node, Python, Go, Rust). If your repo has a `Dockerfile` at its root, arx uses it as-is. Override per service:

```bash
arx -w default -p demo service config set web \
    --build-cmd "..." --start-cmd "..."
```

### Healthchecks

By default arx waits for the container to listen on `PORT`, then `ARX_PORT`, then
`8080`. Use an HTTP endpoint when the app has one:

```bash
arx -w default -p demo config set web \
    --healthcheck http --healthcheck-path /health
```

For no-port long-running services such as Discord bots, disable port probing:

```bash
arx -w default -p demo service create \
    --slug discord-bot --name "Discord Bot" --kind git \
    --repo your-org/discord-bot --branch main \
    --start-cmd "node bot.js" \
    --healthcheck none
```

`--healthcheck none` does not create a public route. If you attach a domain to
such a service, the container must still listen on `PORT` / `ARX_PORT` (or the
fallback `8080`) for Traefik to reach it.

Service variables (`var set`) are available **both at build time and at runtime**.
For auto-detected stacks they appear in your build command as ordinary env vars
(e.g. `process.env.MY_VAR`) — no code change. They ride a BuildKit secret, so
values never land in the image history or layers. In a **custom `Dockerfile`**
you opt in by mounting the secret yourself:

```dockerfile
# syntax=docker/dockerfile:1.7
RUN --mount=type=secret,id=arx_env \
    . /run/secrets/arx_env && npm run build
```

### Monorepos

Point each service at a workspace package with `--root-directory`. arx will detect `turbo.json` / `pnpm-workspace.yaml` / `package.json#workspaces` in an ancestor directory and switch to a workspace-aware build (`pnpm --filter ./apps/web`, `bun --filter`, `npm -w`, or `yarn workspace <name>`). The Docker build context becomes the monorepo root, so a `.dockerignore` at the root is recommended.

The build honors the repo's `packageManager` field (corepack), copies only the workspace `package.json` manifests into a cached dependency-install layer, and installs once — so source-only changes reuse the install cache.

```bash
arx -w default -p demo service create \
    --slug web --name Web --kind git \
    --repo your-org/your-mono --branch main \
    --root-directory apps/web

arx -w default -p demo service create \
    --slug api --name API --kind git \
    --repo your-org/your-mono --branch main \
    --root-directory apps/api
```

Pushes only redeploy services whose `--root-directory` (or `--watch-path` glob) intersects the changed files, so editing `apps/web/**` will not rebuild `api`. Override the default with one or more `--watch-path` glob patterns when needed.

## CLI

```
arx --help
arx <noun> --help
```

`--json` for machine output, `-q/--quiet` to suppress informational messages.

## Web UI

arx is CLI-first, but the daemon also serves a small web dashboard. Once an
admin domain is configured, open `https://arx.<root-domain>` in a browser, sign
in with GitHub, and you can browse workspaces, projects, and services; deploy,
restart, and roll back; stream logs; and edit variables, domains, and server
settings. The UI is a static SPA embedded in the daemon binary — no extra
container, no extra port. It uses the same GitHub login as the CLI; the browser
session is an `HttpOnly` cookie, and cross-site requests to mutating endpoints
are rejected (the CLI's `Bearer` token path is unaffected).

## Outgoing webhooks

arx can POST a signed JSON event to a URL you register whenever a lifecycle
event happens, so you can drive CI, dashboards, chat notifications, or any
automation off your deploys. Webhooks are workspace-scoped and managed by
workspace **admins**.

```
arx -w myws webhook create --url https://example.com/hook
arx -w myws webhook create --url https://example.com/hook --events deployment.failed,backup.failed
arx -w myws webhook list
arx -w myws webhook test <id>
arx -w myws webhook deliveries <id>
arx -w myws webhook redeliver <id> <delivery-id>
arx -w myws webhook enable <id>     # re-arm an auto-disabled endpoint
```

`create` prints a signing secret **once** — save it. Pass `--project <slug>` to
scope an endpoint to one project, and `--events` to subscribe to specific event
types (default `*` = all).

### Event types

`deployment.started`, `deployment.succeeded`, `deployment.failed`,
`deployment.rolling_back`, `deployment.rolled_back`, `deployment.rollback_failed`,
`service.restarting`, `service.restarted`, `service.restart_failed`,
`backup.succeeded`, `backup.failed`, and `test`.

### Payload & verifying the signature

Each delivery is an HTTP `POST` with `Content-Type: application/json` and:

```json
{
  "id": "evt_…",
  "type": "deployment.succeeded",
  "created_at": "2026-05-30T12:00:00Z",
  "workspace": "myws",
  "data": { "project": "api", "service": "web", "environment": "production",
            "deployment_id": "…", "status": "live", "reason": null }
}
```

Headers: `X-Arx-Event`, `X-Arx-Delivery` (stable across retries — use it to
deduplicate), `X-Arx-Timestamp` (Unix seconds), and `X-Arx-Signature-256`.

The signature is `sha256=` + HMAC-SHA256 over the bytes `"<timestamp>.<body>"`
using your signing secret. Verify by recomputing it and rejecting deliveries
whose `X-Arx-Timestamp` is outside a tolerance window (e.g. 5 minutes) to guard
against replay. Failed deliveries are retried with exponential backoff; an
endpoint that fails persistently is auto-disabled (re-enable with
`arx webhook enable`).

### Outbound network policy (SSRF)

The daemon will not deliver to loopback, link-local, cloud metadata
(`169.254.169.254` and equivalents), or its own address — these are blocked at
the resolved-IP level and redirects are not followed. Sending to other hosts on
your own LAN is allowed, since that is a normal self-hosted use case.

## License

Licensed under either [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE) — pick whichever fits your project.
