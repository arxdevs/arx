# Installing arx

This document covers every supported install path. For the 30-second version see the [README](./README.md).

## Requirements

- Linux or macOS host with Docker Engine running (`docker --version`).
- Outbound network to GitHub, ghcr.io, and `8.8.8.8` (UDP/53). arx's domain verification queries Google DNS directly; see the README's network-requirement note.
- For the server box: a hostname pointing at the box's public IP (an A record) if you want public HTTPS via ACME. Local-only / homelab installs without a real domain still work for everything except certificate issuance.

## Server box — full install

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx setup
```

What happens:

1. `install.sh` places the `arx` CLI binary at `~/.local/bin/arx`. The script downloads the matching tarball for your OS/arch from the latest GitHub Release, verifies the executable, and exits.
2. `arx setup` ensures the daemon stack is running (`docker compose up -d` for the `arx` + `traefik` services). It then walks you through:
   - Creating a GitHub App via the manifest flow (browser-driven; see "Headless install" below for the SSH workflow).
   - Recording your admin domain (`arx.<root-domain>`) and ACME email.
   - Creating the first user from the GitHub account that authorised the App.
   - Creating the default workspace.

When `arx setup` returns, the daemon is healthy at `127.0.0.1:7878` and reachable publicly at `https://arx.<root-domain>` (once DNS and certs land).

### Picking the install location

`install.sh` honors `ARX_BIN_DIR` (default `$HOME/.local/bin`) and `ARX_VERSION` (default: the latest GitHub Release tag).

```bash
ARX_BIN_DIR=/usr/local/bin curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sudo sh
ARX_VERSION=v0.1.0 curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
```

### Headless install (SSH-only servers)

If the server has no browser, run `arx setup` with the bundled loopback flag and forward port 7919 from your laptop:

```bash
ssh -L 7919:127.0.0.1:7919 user@server "arx setup"
```

Open the printed `http://127.0.0.1:7919/` URL on your laptop. After GitHub redirects back to that loopback address, the manifest exchange completes over the tunnel and `arx setup` continues on the server.

If port forwarding is not available, run `arx setup --no-browser`. The CLI prints a URL — open it manually on any browser, copy the `code` parameter from the redirect URL bar, and paste it back into the terminal prompt.

## Client laptop

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
arx login --server https://arx.<root-domain>
```

`arx login` opens a browser, completes GitHub OAuth (same App as the server), and stores a Bearer token in `~/.arx/credentials.json`. Subsequent CLI commands transparently use this credential against the configured server.

For headless laptops add `--device`:

```bash
arx login --server https://arx.<root-domain> --device
```

You receive a short code; visit the printed URL in any browser, enter the code, and the CLI completes login when the auth lands.

## Updating

The CLI:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
```

Re-running `install.sh` overwrites the binary in place with the latest release.

The daemon:

```bash
arx server upgrade
```

This pulls `ghcr.io/arxdevs/arx:latest`, recreates the `arx` and `traefik` containers, and runs any pending SQL migrations. Active user service containers are not touched.

## Uninstalling

```bash
arx server uninstall   # tears down the compose stack
rm "$(command -v arx)"
rm -rf ~/.arx
```

`arx server uninstall` stops the `arx` + `traefik` containers and removes them. It does **not** delete the `arx-data` Docker volume (your SQLite database, encrypted variables, and Traefik state) — remove it explicitly with `docker volume rm arx_arx-data` if you really want a clean slate.

## What the install scripts touch

- `install.sh`: writes one file to `${ARX_BIN_DIR:-$HOME/.local/bin}/arx`. No services, no PATH edits, no shell-rc modifications.
- `arx setup`: writes `compose.yml` to `~/.arx/`, brings up the docker compose stack, and stores the session token in `~/.arx/credentials.json`.

Everything else lives inside the `arx-data` Docker volume (database, encryption key, Traefik dynamic config) — there is no on-host state to back up beyond that volume.
