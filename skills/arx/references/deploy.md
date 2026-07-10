# Deploy

Create a project and service, ship it, confirm it went live, and roll back if
needed. All commands need a workspace and project context — export
`ARX_WORKSPACE` and `ARX_PROJECT` first, or pass `-w`/`-p`.

## Standard flow (GitHub repo)

```bash
export ARX_WORKSPACE=default ARX_PROJECT=demo

# 1. Project (a group of services). Skip if it already exists.
arx project create --slug demo --name Demo

# 2. Service from a GitHub repo. arx auto-detects the stack
#    (JVM, Node, Python, Go, Rust) or uses a root Dockerfile as-is.
#    A Node package using Vite with no start script deploys as a static
#    SPA behind nginx (dist/ with index.html fallback) automatically.
arx service create --slug web --name Web --kind git \
    --repo your-org/your-repo --branch main

# 3. (optional) attach a custom domain for public HTTPS
arx domain add web web.your-domain

# 4. Deploy. This is ASYNCHRONOUS — it returns a pending deployment.
arx deploy web

# 5. Confirm it actually went live (do not assume success from step 4).
arx deployments web --json     # look for the newest entry's status: live | failed
```

If `status` is `failed`, read `references/troubleshoot.md`.

## Service kinds

`arx service create --kind <git|image|db>`:

- **git** — build from a GitHub repo. Flags: `--repo <org/repo>`,
  `--branch <name>` (default `main`), `--dockerfile <path>` (explicit Dockerfile),
  `--root-directory <dir>` (monorepo package, e.g. `apps/web`),
  `--watch-path <glob>` (restrict which pushed paths trigger redeploy; repeatable),
  `--build-cmd` / `--start-cmd` (override auto-detection).
- **image** — run a prebuilt image. Flag: `--image <ref>`.
- **db** — a database from a template. Flag:
  `--template <postgres|mysql|mongodb|redis>`. arx provisions a persistent volume
  and credentials; see `references/backups.md` to back it up.

## Monorepos

Point each service at its package with `--root-directory`. arx detects
`turbo.json` / `pnpm-workspace.yaml` / `package.json#workspaces` in an ancestor and
runs a workspace-aware build (`pnpm --filter`, `bun --filter`, `npm -w`,
`yarn workspace`). A push only redeploys services whose `--root-directory` (or
`--watch-path`) intersects the changed files.

```bash
arx service create --slug web --name Web --kind git \
    --repo your-org/mono --branch main --root-directory apps/web
arx service create --slug api --name API --kind git \
    --repo your-org/mono --branch main --root-directory apps/api
```

## Roll back

List deployments to find a prior good one, then roll back to its id:

```bash
arx deployments web --json
arx rollback web <deployment-id>
```

Rollback re-runs that deployment's image (zero-downtime, same as a deploy). It is
not supported for `db` services.

## Notes

- Auto-detected stacks expose service vars (`arx var set`) as ordinary build- and
  run-time env vars. In a custom Dockerfile you opt in by mounting the
  `arx_env` BuildKit secret. See `references/operate.md` for variables.
- A deploy that never reaches `live` leaves the previous version serving — the swap
  only happens after the new container is healthy.
