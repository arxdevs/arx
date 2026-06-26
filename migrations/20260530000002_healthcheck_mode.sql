ALTER TABLE service_env_configs
    ADD COLUMN healthcheck_mode TEXT NOT NULL DEFAULT 'tcp'
    CHECK (healthcheck_mode IN ('tcp', 'http', 'none'));

UPDATE service_env_configs
SET healthcheck_mode = 'http'
WHERE healthcheck_path IS NOT NULL AND length(trim(healthcheck_path)) > 0;
