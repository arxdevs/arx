# Installing arx

A playbook for coding agents. If you are a human and just want to install arx yourself, run the two-line install in the [README](./README.md) instead — this document is written to be followed by an LLM coding agent acting on your behalf.

## Questions to ask the user

Ask one at a time, in the user's language. Wait for an answer before continuing. Skip a question if its precondition does not apply.

1. **Role** — Will this machine run the arx **daemon (server)**, talk to a remote arx (**client**), or **both**?
2. **Install location** — Default is `$HOME/.local/bin`. Is that fine, or use a different path?
3. **Public domain** (server only) — Do you own a hostname whose A record points at this box's public IP? If yes, what is your root domain (e.g. `me.com`)? If no, the daemon still works locally; public HTTPS via ACME will be unavailable until a domain is attached later.
4. **ACME email** (server only, if a public domain was given) — Which email should Let's Encrypt use for certificate notices?
5. **Headless / SSH** (server only) — Are you on this box over SSH with no local browser? If yes, the agent will arrange an SSH tunnel for the GitHub App manifest flow.
6. **Server URL** (client only, when the server is on a different machine) — Where is your arx daemon reachable, e.g. `https://arx.me.com`?
7. **Docker socket access** (server only, last confirmation) — Installing arx grants the daemon root-equivalent access via `/var/run/docker.sock`, opens ports 80, 443, and binds `127.0.0.1:7878`. Continue?

## Steps

### Server install

Run only when role is `server` or `both`.

Preflight (run each, surface failures to the user):

```bash
docker --version
docker info
```

Install the CLI:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
```

If the script prints a `PATH` hint, surface it verbatim — do not edit the user's shell rc files yourself.

Run setup:

```bash
arx setup
```

`arx setup` is interactive. Surface every prompt to the user verbatim. The prompts cover:

- GitHub App manifest creation (browser-driven).
- Admin domain (defaults to `arx.<root-domain>` from question 3).
- ACME email (from question 4).

If the user answered yes to question 5 (headless), tell them to either re-run setup through an SSH tunnel:

```bash
ssh -L 7919:127.0.0.1:7919 user@server arx setup
```

or use the no-browser form:

```bash
arx setup --no-browser
```

In the `--no-browser` form, surface the printed URL and prompt the user to paste the redirect URL's `code` parameter.

### Client install

Run only when role is `client` or `both`.

If `both` was chosen and the server section already installed the CLI on this machine, skip the binary install.

Otherwise:

```bash
curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
```

Log in using the server URL from question 6 (or the `arx.<root-domain>` value from question 3 if `both`):

```bash
arx login --server <server-url>
```

For headless laptops, use:

```bash
arx login --server <server-url> --device
```

Surface the printed code and URL verbatim.

## Verification

Server (after `arx setup` returns):

```bash
curl -s http://127.0.0.1:7878/health
```

Expected output: `ok`. Report success or failure to the user.

Client (after `arx login`):

```bash
arx whoami
```

Expected output: JSON with the user's `github_login`. Report success or failure.

## Things the agent must not do

- Do not edit `~/.bashrc`, `~/.zshrc`, `~/.profile`, or any shell rc file without explicit consent.
- Do not run `sudo` unless the user explicitly chose a system-wide install path (e.g. `/usr/local/bin`).
- Do not skip or paraphrase `arx setup` prompts. They configure the GitHub App, admin domain, and ACME email — the agent must surface them verbatim and pass through the user's answers.
- Do not invent answers to the questions above. Ask the user; if they say "you decide", pick the documented default and tell them which.
- Do not run any uninstall command unless the user explicitly asked for uninstallation.

## Environment variables

- `ARX_BIN_DIR` — override install destination. Set only if the user picked a non-default path in question 2.
- `ARX_VERSION` — pin a specific release tag. Set only if the user explicitly requested a version.

## Uninstall

Run only when the user explicitly asks to uninstall.

Stop the daemon stack and remove the CLI:

```bash
arx server uninstall
rm "$(command -v arx)"
rm -rf ~/.arx
```

The `arx-data` Docker volume (SQLite DB, encryption key, Traefik state) is preserved by default. Confirm with the user before removing it:

```bash
docker volume rm arx_arx-data
```
