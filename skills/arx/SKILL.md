---
name: arx
description: Deploy and operate apps on a self-hosted arx PaaS through the `arx` CLI. Use this whenever the user wants to ship code to their own server or home server, deploy a GitHub repo or Docker image, stand up a database, manage services / environments / env vars / custom domains, view container logs, exec into a running container, roll back or restart a deployment, run or schedule database backups, or wire up outgoing webhooks — even if they don't say the word "arx". Triggers on phrases like "deploy to my server", "deploy to my home server", "put this on my box", "self-hosted deploy", or any mention of arx, `arx`, or an arx workspace/project/service.
license: MIT OR Apache-2.0
metadata:
  project: arx
  homepage: https://github.com/arxdevs/arx
---

# arx

arx is a self-hosted PaaS for home servers. You connect a GitHub repo (or point at a
Docker image / database template); arx builds it, runs it as a container, routes
traffic through Traefik with automatic ACME TLS, and keeps it alive across
zero-downtime swaps. You drive everything through the `arx` CLI, which talks to a
daemon over HTTP.

Operate arx on the user's behalf with this skill. For any task beyond the basics
below, read the focused file in `references/` rather than guessing flags — the
references mirror the real CLI surface.

## Mental model (read first)

Everything in arx lives in a four-level hierarchy, and every command operates at one
level of it. Knowing the level tells you which `-w/-p/-e` flags a command needs.

```
workspace            tenant boundary               (-w / ARX_WORKSPACE)
└─ project           a group of related services   (-p / ARX_PROJECT)
   └─ service        one deployable unit            (positional <service>)
      └─ environment "production" by default        (-e / ARX_ENV)
```

- A **service** has a `source`: `git` (build from a GitHub repo), `image` (run a
  prebuilt Docker image), or `db` (a database from a template: postgres / mysql /
  mongodb / redis).
- A **deployment** is one attempt to run a service. It moves
  `pending → deploying → live`, or ends `failed`. The previous version is only
  retired (`superseded`) after the new one is healthy — that is the zero-downtime
  guarantee, and it means a `deploy` returning does not yet mean "live".
- **Routing**: each service is served at `<service>.<project>.<root-domain>` via
  Traefik; the admin API at `arx.<root-domain>`. TLS is automatic via ACME once a
  root domain is attached.

## CLI basics

- Context flags (each also an env var): `-w/--workspace` (`ARX_WORKSPACE`),
  `-p/--project` (`ARX_PROJECT`), `-e/--env` (`ARX_ENV`, default `production`),
  `--server` (`ARX_SERVER`, default `http://127.0.0.1:7878`). Output: `--json`,
  `-q/--quiet`.
- Export the context once at the start of a task so later commands inherit it —
  it's less error-prone than repeating flags:
  ```bash
  export ARX_WORKSPACE=default ARX_PROJECT=demo
  ```
- Use `--json` whenever you need to parse output (e.g. to read a deployment's
  `status` or grab an id). Plain output is for showing the user.
- Auth: commands carry a token from `arx login`. If a command returns
  unauthorized, the user must `arx login` first (or `arx login --device` on a
  headless box). Don't try to route around auth.

## Command map — which reference to read

| You want to… | Commands | Reference |
| --- | --- | --- |
| Stand up an arx server, or log in a client | `arx setup`, `arx login` | `references/setup.md` |
| Create project/service and deploy a repo, image, or DB; verify; roll back | `arx project/service create`, `arx deploy`, `arx deployments`, `arx rollback` | `references/deploy.md` |
| Operate a running service: logs, build logs, exec, restart, env vars, domains, build/start cmds | `arx logs`, `arx build-logs`, `arx exec`, `arx restart`, `arx var`, `arx domain`, `arx service config` | `references/operate.md` |
| Back up / restore a database, schedule backups | `arx backup` | `references/backups.md` |
| Send event notifications to external URLs | `arx webhook` | `references/webhooks.md` |
| A deploy failed or a service is unreachable | — | `references/troubleshoot.md` |

## arx-specific gotchas (why they matter)

- **A `deploy` is asynchronous.** `arx deploy <service>` returns immediately with a
  `pending` deployment; it does not block until live. Confirm the real outcome with
  `arx deployments <service> --json` and look for `status: live` (or `failed`).
  Never report success from the `deploy` call alone.
- **Verify against reality.** After any change, check `arx deployments` / `arx logs`
  to confirm the service is actually live and serving — assumptions about what "should"
  have happened are how silent breakage slips through.
- **`restart` re-runs the current image; `deploy` rebuilds.** Use `restart` to
  recycle a container (pick up an env var change, clear a stuck process); use
  `deploy` to ship new code.
- **Public HTTPS needs a root domain.** It's set during `arx setup`. Without one the
  service still runs and is reachable locally, but ACME TLS and public URLs are
  unavailable — so don't promise a public `https://` URL until a domain exists.
- **A `git` service needs the arx GitHub App to reach the repo.** Public repos work
  anonymously; private repos require the App installed on that repo (done during
  `arx setup`). A clone/auth failure on deploy usually means the App isn't installed.
- **Never expose `127.0.0.1:7878`.** The daemon binds loopback on purpose and Traefik
  fronts it; don't suggest opening that port or pointing a public domain straight at it.
