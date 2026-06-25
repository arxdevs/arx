# Setup & login

Get an arx server running, or point a client at an existing one. Full end-user
install guidance lives in the arx repo's `install.md`; this is the operational
summary.

## Roles

A machine can be a **server** (runs the arx daemon + Traefik via Docker), a
**client** (just the `arx` CLI talking to a remote daemon), or **both**. Pick based
on what the user is doing.

## Server setup

Run on the box that will host apps. Requires Docker.

```bash
arx setup
```

`setup` walks through: public IP, root domain (for public HTTPS), ACME email, and
the GitHub App connection (so arx can clone private repos). Useful flags:

- `--headless` / `--no-browser` — on a remote box over SSH with no local browser;
  arx arranges an SSH tunnel for the GitHub App manifest flow.
- `--root-domain <domain>` — the domain whose A record points at this box. Without
  it, services run locally but get no public HTTPS.
- `--admin-domain <domain>`, `--acme-email <email>`, `--public-ip <ip>` — supply
  these non-interactively if known.

After setup, the daemon listens on `127.0.0.1:7878` and Traefik fronts ports 80/443.

## Client login

On a machine that talks to a remote daemon, or to authenticate the CLI locally:

```bash
arx login                 # opens a browser for GitHub OAuth
arx login --device        # headless: prints a device code to enter in a browser
arx login --token <token> # non-interactive with a known token
```

Point the CLI at a remote server with `--server` or `ARX_SERVER`:

```bash
export ARX_SERVER=https://arx.your-domain
arx login --device
arx whoami                # confirm you're authenticated
```

## Verify

```bash
arx whoami                # who am I / am I logged in
arx workspace list        # the daemon is reachable and authorized
```

If `workspace list` returns unauthorized, run `arx login` first. If it can't reach
the server at all, check `ARX_SERVER` / `--server` and that the daemon is running.
