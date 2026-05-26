-- Singleton settings row for daemon-level configuration set via setup.

CREATE TABLE server_settings (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    admin_domain    TEXT,
    acme_email      TEXT,
    public_ip       TEXT,
    updated_at      TEXT NOT NULL
);

INSERT INTO server_settings (id, admin_domain, acme_email, public_ip, updated_at)
VALUES (1, NULL, NULL, NULL, datetime('now'));
