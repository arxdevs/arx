# Troubleshooting

A diagnostic checklist for when a deploy fails or a service is unreachable. Work
top-down: confirm what state things are actually in before changing anything.

## A deploy ended `failed`

```bash
arx deployments web --json     # confirm the newest status is `failed`
arx logs web --tail 200        # the build/runtime error is almost always here
```

Common causes:

- **Build failure** — bad build command or missing dependency. Inspect the logs;
  override with `arx service config set web --build-cmd "..."` and redeploy.
- **Healthcheck never passed** — the container started but didn't become healthy
  (wrong start command, app crashed, wrong port). Check logs for a crash;
  fix `--start-cmd` or the app, then redeploy. The previous version keeps serving in
  the meantime.
- **Repo clone / auth failure** (git services) — the arx GitHub App isn't installed
  on a private repo. Re-run the GitHub App step in `arx setup`, or make the repo
  public.
- **Pre-deploy command failed** (e.g. a migration) — the logs show its stderr; fix
  the command or the migration and redeploy.

## A service is unreachable over HTTP/HTTPS

- Is it actually live? `arx deployments web --json` → `status: live`. If not, treat
  it as a failed deploy above.
- Is a domain attached and pointing here? `arx domain list web`, and confirm the
  domain's A record resolves to this box.
- Is there a root domain configured at all? Without one (set in `arx setup`), there
  is no public HTTPS — the service is only reachable locally.
- TLS pending — ACME issuance takes a moment after a domain first resolves. Give it
  a minute, then retry.

## Commands return unauthorized

Run `arx login` (or `arx login --device` on a headless box). If a remote server,
confirm `ARX_SERVER` / `--server` points at the right daemon.

## Can't reach the daemon at all

- Check `ARX_SERVER` / `--server`. Locally it defaults to `http://127.0.0.1:7878`.
- Confirm the daemon is running on the server box.
- Do **not** try to reach `127.0.0.1:7878` from outside the host or open that port —
  it binds loopback by design; Traefik fronts public traffic.

## A service won't pick up a config or env change

Variables and `service config` changes apply on the next run, not to the live
container. Run `arx restart web` (no rebuild) for an env/var change, or
`arx deploy web` (rebuild) for code or build-command changes — then re-verify with
`arx deployments` / `arx logs`.
