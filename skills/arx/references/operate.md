# Operate a running service

Day-to-day management of a live service. All commands take a workspace + project
context (`-w`/`-p` or `ARX_WORKSPACE`/`ARX_PROJECT`) and the service slug.

## Logs

```bash
arx logs web                  # recent logs
arx logs web -f               # follow (stream)
arx logs web --tail 200       # last N lines
arx logs web --since 1h       # newer than a relative duration: 1h, 30m, 10s, 2d
```

Logs are the first place to look when a service misbehaves at runtime. Runtime
logs come straight from Docker (`docker logs`), so arx stores nothing for them.

## Build logs

Build logs are the `docker build` output for a deployment. Unlike runtime logs,
Docker does not retain them, so arx captures and stores them per deployment —
including past and failed builds.

```bash
arx build-logs web                       # build log of the latest deployment
arx build-logs web -f                    # follow a build live as it runs
arx build-logs web --deployment <id>     # a specific past deployment (see `arx deployments`)
```

Build logs are the first place to look when a deploy is `failed`.

## Exec into the container

Run a one-off command, or open an interactive shell, in the live container:

```bash
arx exec web -- printenv          # run a command (note the `--` before the command)
arx exec web -- sh                # interactive shell
```

Use this to inspect runtime state — but prefer fixing config (vars, build/start
commands) and redeploying over hand-editing a running container, since containers
are replaced on the next deploy.

## Restart

```bash
arx restart web
```

Re-runs the **current** image with no rebuild. Use it to recycle a container — e.g.
after changing an env var, or to clear a wedged process. To ship new code, use
`arx deploy` (a rebuild) instead.

## Environment variables

Variables are available **both at build time and at runtime**.

```bash
arx var list web
arx var set web DATABASE_URL=postgres://...        # plaintext, visible in `var list`
arx var set web API_KEY=secret --sealed            # sealed: write-only, never shown back
arx var unset web OLD_KEY
arx var import web .env                             # bulk from a dotenv file
arx var import web .env --sealed-all --overwrite   # seal all, replace existing
```

A var change does not affect a running container until the next `arx restart` or
`arx deploy`. Use `--sealed` for secrets so their values are never returned by the
API or printed.

## Custom domains

```bash
arx domain list web
arx domain add web web.your-domain        # point an A record at the box first
arx domain remove <domain-id>             # the id comes from `domain list`
```

TLS is issued automatically via ACME once the domain resolves to the server and a
root domain is configured (see `references/setup.md`).

## Build / start commands

Override the auto-detected build or start command for a `git` service:

```bash
arx service config set web --build-cmd "npm run build" --start-cmd "node dist/server.js"
arx service config set web --start-cmd ""     # an empty string clears the override
```

Changes take effect on the next `arx deploy`.
