ALTER TABLE services ADD COLUMN restart_policy TEXT NOT NULL DEFAULT 'unless-stopped';
