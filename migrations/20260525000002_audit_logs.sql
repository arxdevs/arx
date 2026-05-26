-- Audit log of meaningful actions taken in arx.
-- Retention is enforced by a background cleanup task (90 days default).

CREATE TABLE audit_logs (
    id          TEXT PRIMARY KEY NOT NULL,
    actor_id    TEXT REFERENCES users(id) ON DELETE SET NULL,
    action      TEXT NOT NULL,
    target      TEXT NOT NULL,
    metadata    TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL
);

CREATE INDEX audit_logs_created_idx ON audit_logs(created_at DESC);
CREATE INDEX audit_logs_actor_idx ON audit_logs(actor_id);
