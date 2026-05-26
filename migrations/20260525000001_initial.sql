-- arx initial schema.
-- All IDs are UUIDv7 stored as TEXT (lowercase hex with dashes).
-- All timestamps are RFC3339 strings stored as TEXT.
-- All booleans are INTEGER 0/1.
-- All JSON payloads are TEXT (validated by application code).

PRAGMA foreign_keys = ON;

-- --------------------------------------------------------------------------
-- Identity
-- --------------------------------------------------------------------------

CREATE TABLE users (
    id              TEXT PRIMARY KEY NOT NULL,
    -- GitHub identity is filled in after OAuth completes. NULL during
    -- bootstrap (pre-GitHub-App) and once the user signs in via GitHub we
    -- backfill these columns.
    github_login    TEXT UNIQUE COLLATE NOCASE,
    github_user_id  INTEGER UNIQUE,
    display_name    TEXT NOT NULL,
    avatar_url      TEXT,
    created_at      TEXT NOT NULL
);

-- Long-lived session tokens (CLI + browser). One row per active session.
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY NOT NULL,         -- session UUID
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,              -- sha256 of token
    label           TEXT,                              -- "cli on laptop", "browser"
    created_at      TEXT NOT NULL,
    last_used_at    TEXT NOT NULL,
    expires_at      TEXT                               -- NULL = no expiry
);

CREATE INDEX sessions_user_idx ON sessions(user_id);

-- --------------------------------------------------------------------------
-- Workspaces
-- --------------------------------------------------------------------------

CREATE TABLE workspaces (
    id          TEXT PRIMARY KEY NOT NULL,
    slug        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE workspace_members (
    id              TEXT PRIMARY KEY NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    created_at      TEXT NOT NULL,
    UNIQUE (workspace_id, user_id)
);

-- Pending invites for users not yet signed in.
-- Becomes a workspace_members row when the GitHub user signs in.
CREATE TABLE workspace_invites (
    id              TEXT PRIMARY KEY NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    github_login    TEXT NOT NULL COLLATE NOCASE,
    role            TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    invited_by      TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (workspace_id, github_login)
);

CREATE INDEX workspace_members_user_idx ON workspace_members(user_id);

-- --------------------------------------------------------------------------
-- Projects, environments, services
-- --------------------------------------------------------------------------

CREATE TABLE projects (
    id              TEXT PRIMARY KEY NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL COLLATE NOCASE,
    name            TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (workspace_id, slug)
);

CREATE TABLE environments (
    id          TEXT PRIMARY KEY NOT NULL,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug        TEXT NOT NULL COLLATE NOCASE,
    name        TEXT NOT NULL,
    is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at  TEXT NOT NULL,
    UNIQUE (project_id, slug)
);

-- Only one default environment per project.
CREATE UNIQUE INDEX environments_default_unique
    ON environments(project_id) WHERE is_default = 1;

CREATE TABLE services (
    id          TEXT PRIMARY KEY NOT NULL,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug        TEXT NOT NULL COLLATE NOCASE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('git_source', 'docker_image', 'db_template')),
    source      TEXT NOT NULL,  -- JSON; shape depends on kind
    created_at  TEXT NOT NULL,
    UNIQUE (project_id, slug)
);

-- Per (service × environment) configuration. One row per pairing.
CREATE TABLE service_env_configs (
    service_id                    TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    environment_id                TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    cpu_limit                     REAL,           -- cores; NULL = unlimited
    memory_limit_mb               INTEGER,        -- MiB; NULL = unlimited
    healthcheck_path              TEXT,           -- NULL = port-listen check
    healthcheck_timeout_seconds   INTEGER NOT NULL DEFAULT 60,
    current_deployment_id         TEXT,           -- FK added implicitly; deployment may not exist yet at first
    PRIMARY KEY (service_id, environment_id)
);

-- --------------------------------------------------------------------------
-- Variables (env vars), encrypted at rest
-- --------------------------------------------------------------------------

CREATE TABLE variables (
    id                TEXT PRIMARY KEY NOT NULL,
    service_id        TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    environment_id    TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    key               TEXT NOT NULL,
    value_ciphertext  BLOB NOT NULL,
    value_nonce       BLOB NOT NULL,
    sealed            INTEGER NOT NULL DEFAULT 0 CHECK (sealed IN (0, 1)),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE (service_id, environment_id, key)
);

CREATE INDEX variables_service_env_idx ON variables(service_id, environment_id);

-- --------------------------------------------------------------------------
-- Domains
-- --------------------------------------------------------------------------

CREATE TABLE domains (
    id              TEXT PRIMARY KEY NOT NULL,
    service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    hostname        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    verified        INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    cert_status     TEXT NOT NULL DEFAULT 'pending'
                    CHECK (cert_status IN ('pending', 'issued', 'failed')),
    created_at      TEXT NOT NULL
);

CREATE INDEX domains_service_env_idx ON domains(service_id, environment_id);

-- --------------------------------------------------------------------------
-- Deployments
-- --------------------------------------------------------------------------

CREATE TABLE deployments (
    id                  TEXT PRIMARY KEY NOT NULL,
    service_id          TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    environment_id      TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    status              TEXT NOT NULL
                        CHECK (status IN ('pending', 'building', 'deploying',
                                          'live', 'failed', 'superseded', 'rolledback')),
    image_ref           TEXT,
    commit_sha          TEXT,
    variables_snapshot  TEXT NOT NULL DEFAULT '{}',  -- JSON
    container_id        TEXT,
    error               TEXT,
    created_at          TEXT NOT NULL,
    finished_at         TEXT
);

CREATE INDEX deployments_service_env_idx ON deployments(service_id, environment_id, created_at DESC);

-- --------------------------------------------------------------------------
-- Backups (per DB service)
-- --------------------------------------------------------------------------

CREATE TABLE backup_schedules (
    service_id          TEXT PRIMARY KEY REFERENCES services(id) ON DELETE CASCADE,
    cron_expression     TEXT NOT NULL DEFAULT '0 3 * * *',  -- daily 03:00
    retention_count     INTEGER NOT NULL DEFAULT 7,
    storage             TEXT NOT NULL DEFAULT 'local' CHECK (storage IN ('local', 's3')),
    s3_config_id        TEXT,                                -- workspace-level s3 config; nullable
    enabled             INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

CREATE TABLE backups (
    id              TEXT PRIMARY KEY NOT NULL,
    service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    size_bytes      INTEGER NOT NULL,
    storage_uri     TEXT NOT NULL,   -- e.g. 'file:///var/lib/arx/backups/<svc>/2026-05-25.dump' or 's3://...'
    created_at      TEXT NOT NULL
);

CREATE INDEX backups_service_idx ON backups(service_id, created_at DESC);

CREATE TABLE s3_configs (
    id              TEXT PRIMARY KEY NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    endpoint        TEXT NOT NULL,
    region          TEXT NOT NULL,
    bucket          TEXT NOT NULL,
    access_key_id   TEXT NOT NULL,
    -- secret stored encrypted via same master key as variables
    secret_ciphertext   BLOB NOT NULL,
    secret_nonce        BLOB NOT NULL,
    created_at      TEXT NOT NULL
);

-- --------------------------------------------------------------------------
-- GitHub App + webhooks
-- --------------------------------------------------------------------------

-- Single row table holding the GitHub App credentials created via manifest flow.
CREATE TABLE github_app (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton
    app_id              INTEGER NOT NULL,
    slug                TEXT NOT NULL,
    name                TEXT NOT NULL,
    client_id           TEXT NOT NULL,
    client_secret_ct    BLOB NOT NULL,
    client_secret_nonce BLOB NOT NULL,
    webhook_secret_ct   BLOB NOT NULL,
    webhook_secret_nonce BLOB NOT NULL,
    private_key_ct      BLOB NOT NULL,
    private_key_nonce   BLOB NOT NULL,
    html_url            TEXT NOT NULL,
    created_at          TEXT NOT NULL
);

CREATE TABLE github_installations (
    id                  INTEGER PRIMARY KEY,   -- GitHub installation id
    account_login       TEXT NOT NULL,
    account_type        TEXT NOT NULL,         -- 'User' or 'Organization'
    workspace_id        TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
    created_at          TEXT NOT NULL
);

CREATE TABLE webhook_events (
    id              TEXT PRIMARY KEY NOT NULL,
    source          TEXT NOT NULL,           -- 'github'
    event_type      TEXT NOT NULL,           -- 'push', 'ping', ...
    delivery_id     TEXT,                    -- X-GitHub-Delivery; for idempotency
    payload         TEXT NOT NULL,           -- raw JSON
    processed       INTEGER NOT NULL DEFAULT 0 CHECK (processed IN (0, 1)),
    error           TEXT,
    received_at     TEXT NOT NULL,
    processed_at    TEXT
);

CREATE UNIQUE INDEX webhook_events_delivery_idx
    ON webhook_events(source, delivery_id) WHERE delivery_id IS NOT NULL;
CREATE INDEX webhook_events_received_idx ON webhook_events(received_at DESC);
