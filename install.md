# Installing arx

This document is written for a coding agent (Claude Code, Codex, Cursor, Copilot, etc.) to follow on the user's behalf.

If you are a human, you can run the install yourself in two lines (see README), or paste the following into your agent:

> Read https://raw.githubusercontent.com/arxdevs/arx/main/install.md and install arx for me. Ask me whatever you need.

The agent should then follow the protocol below.

---

## Agent protocol

### Step 1 — Role check

Ask the user, in their language:

> Are we installing on the **server box** (the machine that will run the arx daemon and host deployments), the **client** (your laptop that issues commands to a remote arx), or both on the same machine?

Do not proceed until the user answers. Record the role. Continue with the matching section(s) below.

### Step 2 — Server install

Only run this section when the role is `server` or `both`.

1. Confirm with the user before proceeding. State plainly:
   > Installing arx grants the daemon root-equivalent access via `/var/run/docker.sock`. It will create a docker compose stack (`arx` + `traefik`) and bind ports 80, 443, and `127.0.0.1:7878`. Continue?

2. Check prerequisites with the user (run the commands, report results, ask if any check fails):
   - `docker --version` — Docker must be installed and reachable.
   - `docker info` succeeds — Docker daemon is running.
   - The user must own a hostname whose A record points at this box (for public HTTPS via ACME). Local-only installs without a domain still work for everything except certificate issuance — ask the user whether they have a public domain to attach now or will attach one later.

3. Run the installer:
   ```bash
   curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
   ```
   If `~/.local/bin` is not on the user's `PATH`, the script prints a hint — surface that hint to the user verbatim.

4. Ask the user for the **root domain** before running setup. Example: `me.com`. arx's admin domain defaults to `arx.<root-domain>` (e.g. `arx.me.com`). Skip if the user said they will attach a domain later.

5. Ask the user for the **ACME email** (used by Let's Encrypt). Skip if no domain is set.

6. Run `arx setup`. This is an interactive flow:
   - It opens a browser to create a GitHub App. If the box is headless or the user is over SSH, tell the user to either re-run inside an SSH tunnel —
     ```bash
     ssh -L 7919:127.0.0.1:7919 user@server arx setup
     ```
     — or use `arx setup --no-browser` and paste the redirect URL's `code` query parameter when prompted.
   - Surface every prompt to the user verbatim. Do not paraphrase or skip.

7. Verify the daemon is up:
   ```bash
   curl -s http://127.0.0.1:7878/health
   ```
   Expected output: `ok`. Report this to the user.

### Step 3 — Client install

Only run this section when the role is `client` or `both`.

1. If this is the same machine where the server was just installed, skip the binary install — the CLI is already in place.

2. Otherwise run:
   ```bash
   curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
   ```

3. Ask the user for the **server URL**. Example: `https://arx.me.com`. If they just installed the server, suggest the same `arx.<root-domain>` value.

4. Run:
   ```bash
   arx login --server <server-url>
   ```
   If the user is on a headless machine, run with `--device` instead and surface the printed code + URL to the user verbatim.

5. Verify:
   ```bash
   arx whoami
   ```
   Expected output: a JSON object with the user's `github_login`. Report to the user.

### Step 4 — Done

State to the user:

> arx is installed. To deploy your first service, run `arx -w default project create --slug demo --name Demo`, then `arx --help` for the rest.

---

## Variables the agent may consult

- `ARX_BIN_DIR` — install destination (default `$HOME/.local/bin`). Ask the user if they want a different path, otherwise leave default.
- `ARX_VERSION` — pin a specific release tag (default: latest). Only override if the user explicitly requests a version.

## Things the agent must not do

- Do not edit the user's shell config files (`~/.bashrc`, `~/.zshrc`, etc.) without explicit consent. If `~/.local/bin` is not on `PATH`, ask the user how they want to fix it.
- Do not install via `sudo` unless the user explicitly chose a system-wide path like `/usr/local/bin`.
- Do not run `arx server uninstall`, `docker volume rm`, or any cleanup command unless the user explicitly asked for uninstallation.
- Do not skip prompts inside `arx setup`. They configure the GitHub App, admin domain, and ACME email — the agent must not guess values.

## Uninstall

Only run when the user explicitly asks:

```bash
arx server uninstall      # stops + removes arx and traefik containers
rm "$(command -v arx)"    # removes the CLI binary
rm -rf ~/.arx             # removes client credentials and compose stack file
```

The `arx-data` Docker volume (SQLite DB, encryption key, Traefik state) is preserved. Confirm with the user before removing it:

```bash
docker volume rm arx_arx-data
```
