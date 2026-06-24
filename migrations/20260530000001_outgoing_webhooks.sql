-- Outgoing webhooks: user-registered endpoints that receive signed JSON POSTs
-- when lifecycle events (deploy, restart, rollback, backup) occur.
--
-- This is distinct from the incoming `webhook_events` table (GitHub -> arx).
-- The `outgoing_webhook_` prefix avoids that collision.

CREATE TABLE outgoing_webhook_endpoints (
    id                    TEXT PRIMARY KEY NOT NULL,
    workspace_id          TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- NULL = all projects in the workspace; reserved for future per-project scoping.
    project_id            TEXT REFERENCES projects(id) ON DELETE CASCADE,
    -- Channel kind. Only 'webhook' is implemented now; future: 'slack', 'discord', 'email'.
    kind                  TEXT NOT NULL DEFAULT 'webhook',
    url                   TEXT NOT NULL,
    -- Per-kind non-secret config (mirrors services.source tagged-JSON convention).
    config                TEXT NOT NULL DEFAULT '{}',
    -- Encrypted credential JSON (ChaCha20-Poly1305, two-column ct+nonce like variables).
    -- For kind=webhook this holds {"signing_secret": "..."}.
    secret_ct             BLOB NOT NULL,
    secret_nonce          BLOB NOT NULL,
    -- JSON array of subscribed event types, or ["*"] for all.
    events                TEXT NOT NULL DEFAULT '["*"]',
    active                INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    consecutive_failures  INTEGER NOT NULL DEFAULT 0,
    first_failure_at      TEXT,
    disabled_reason       TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

CREATE INDEX outgoing_webhook_endpoints_ws_idx
    ON outgoing_webhook_endpoints(workspace_id, project_id);

CREATE TABLE outgoing_webhook_deliveries (
    id              TEXT PRIMARY KEY NOT NULL,  -- == X-Arx-Delivery, stable across retries
    endpoint_id     TEXT NOT NULL REFERENCES outgoing_webhook_endpoints(id) ON DELETE CASCADE,
    event_id        TEXT NOT NULL,              -- envelope id (evt_...), receiver idempotency
    event_type      TEXT NOT NULL,
    payload         TEXT NOT NULL,              -- serialized envelope body (reused on redeliver)
    -- pending -> in_flight -> success | failed (failed = exhausted or permanent)
    status          TEXT NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT,
    lease_until     TEXT,                       -- in_flight claim lease (crash recovery)
    response_status INTEGER,                    -- HTTP status code only; never the body
    response_size   INTEGER,                    -- diagnostic only
    error           TEXT,                       -- short classified reason, no sensitive data
    created_at      TEXT NOT NULL,
    delivered_at    TEXT,
    exhausted_at    TEXT                         -- set on dead-letter; pruner retains these
);

CREATE INDEX outgoing_webhook_deliveries_due_idx
    ON outgoing_webhook_deliveries(status, next_attempt_at);
CREATE INDEX outgoing_webhook_deliveries_endpoint_idx
    ON outgoing_webhook_deliveries(endpoint_id);
