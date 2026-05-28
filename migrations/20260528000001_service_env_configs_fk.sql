CREATE TABLE service_env_configs_new (
    service_id                    TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    environment_id                TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    cpu_limit                     REAL,
    memory_limit_mb               INTEGER,
    healthcheck_path              TEXT,
    healthcheck_timeout_seconds   INTEGER NOT NULL DEFAULT 60,
    current_deployment_id         TEXT REFERENCES deployments(id) ON DELETE SET NULL,
    PRIMARY KEY (service_id, environment_id)
);

INSERT INTO service_env_configs_new
    (service_id, environment_id, cpu_limit, memory_limit_mb,
     healthcheck_path, healthcheck_timeout_seconds, current_deployment_id)
SELECT service_id, environment_id, cpu_limit, memory_limit_mb,
       healthcheck_path, healthcheck_timeout_seconds, current_deployment_id
FROM service_env_configs;

DROP TABLE service_env_configs;
ALTER TABLE service_env_configs_new RENAME TO service_env_configs;
