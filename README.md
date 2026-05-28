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

arx auto-detects the build stack (Gradle/Maven JVM, Node, Python, Go). If your repo has a `Dockerfile` at its root, arx uses it as-is. Override per service:

```bash
arx -w default -p demo service config set web \
    --build-cmd "..." --start-cmd "..."
```

### Monorepos

Point each service at a workspace package with `--root-directory`. arx will detect `turbo.json` / `pnpm-workspace.yaml` / `package.json#workspaces` in an ancestor directory and switch to a workspace-aware build (`pnpm --filter ./apps/web`, `bun --filter`, `npm -w`, or `yarn workspace <name>`). The Docker build context becomes the monorepo root, so a `.dockerignore` at the root is recommended.

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

## License

Licensed under either [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE) — pick whichever fits your project.
