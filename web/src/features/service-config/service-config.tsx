import { useEffect, useState } from "react";
import {
  serviceApi,
  type Service,
  type EnvConfig,
} from "@/entities/service";
import {
  Card,
  Field,
  Select,
  Button,
  Spinner,
  ErrorMessage,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import styles from "./service-config.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  service: Service;
  onSaved: () => void;
}

const RESTART_POLICIES = [
  "no",
  "unless-stopped",
  "always",
  "on-failure",
].map((v) => ({ value: v, label: v }));

const HEALTHCHECK_MODES = ["tcp", "http", "none"].map((v) => ({
  value: v,
  label: v,
}));

export function ServiceConfig({ ws, proj, svc, service, onSaved }: Props) {
  const config = useQuery<EnvConfig>(
    () => serviceApi.getConfig(ws, proj, svc),
    [ws, proj, svc],
  );

  const [build, setBuild] = useState(service.build_command ?? "");
  const [start, setStart] = useState(service.start_command ?? "");
  const [preDeploy, setPreDeploy] = useState(service.pre_deploy_command ?? "");
  const [restart, setRestart] = useState(service.restart_policy);

  const [cpu, setCpu] = useState("");
  const [memory, setMemory] = useState("");
  const [hcMode, setHcMode] = useState("tcp");
  const [hcPath, setHcPath] = useState("");
  const [hcTimeout, setHcTimeout] = useState("");

  useEffect(() => {
    if (config.data) {
      setCpu(config.data.cpu_limit?.toString() ?? "");
      setMemory(config.data.memory_limit_mb?.toString() ?? "");
      setHcMode(config.data.healthcheck_mode);
      setHcPath(config.data.healthcheck_path ?? "");
      setHcTimeout(config.data.healthcheck_timeout_seconds.toString());
    }
  }, [config.data]);

  const saveService = useMutation(
    () =>
      serviceApi.patch(ws, proj, svc, {
        build_command: build || null,
        start_command: start || null,
        pre_deploy_command: preDeploy || null,
        restart_policy: restart,
      }),
    onSaved,
  );

  const saveEnv = useMutation(() =>
    serviceApi.patchConfig(ws, proj, svc, {
      cpu_limit: cpu ? Number(cpu) : null,
      memory_limit_mb: memory ? Number(memory) : null,
      healthcheck_mode: hcMode,
      healthcheck_path: hcPath || null,
      healthcheck_timeout_seconds: hcTimeout ? Number(hcTimeout) : undefined,
    }),
  );

  return (
    <div className="arx-stack">
      <Card>
        <h3 className="arx-section-title">Build &amp; runtime</h3>
        <div className={styles.form}>
          <Field
            label="Build command"
            value={build}
            placeholder="auto-detected"
            onChange={(e) => setBuild(e.target.value)}
          />
          <Field
            label="Start command"
            value={start}
            placeholder="auto-detected"
            onChange={(e) => setStart(e.target.value)}
          />
          <Field
            label="Pre-deploy command"
            value={preDeploy}
            placeholder="e.g. migrations"
            onChange={(e) => setPreDeploy(e.target.value)}
          />
          <Select
            label="Restart policy"
            options={RESTART_POLICIES}
            value={restart}
            onChange={(e) => setRestart(e.target.value)}
          />
          {saveService.error && (
            <ErrorMessage message={saveService.error.message} />
          )}
          <div className={styles.actions}>
            <Button
              variant="primary"
              loading={saveService.loading}
              onClick={() => saveService.run(undefined)}
            >
              Save
            </Button>
          </div>
        </div>
      </Card>

      <Card>
        <h3 className="arx-section-title">Resources &amp; healthcheck</h3>
        {config.loading && <Spinner label="loading config" />}
        {config.error && <ErrorMessage message={config.error.message} />}
        {config.data && (
          <div className={styles.form}>
            <div className={styles.grid2}>
              <Field
                label="CPU limit"
                value={cpu}
                placeholder="e.g. 0.5"
                onChange={(e) => setCpu(e.target.value)}
              />
              <Field
                label="Memory (MB)"
                value={memory}
                placeholder="e.g. 512"
                onChange={(e) => setMemory(e.target.value)}
              />
            </div>
            <Select
              label="Healthcheck mode"
              options={HEALTHCHECK_MODES}
              value={hcMode}
              onChange={(e) => setHcMode(e.target.value)}
            />
            {hcMode === "http" && (
              <Field
                label="Healthcheck path"
                value={hcPath}
                placeholder="/health"
                onChange={(e) => setHcPath(e.target.value)}
              />
            )}
            <Field
              label="Healthcheck timeout (s)"
              value={hcTimeout}
              onChange={(e) => setHcTimeout(e.target.value)}
            />
            {saveEnv.error && <ErrorMessage message={saveEnv.error.message} />}
            <div className={styles.actions}>
              <Button
                variant="primary"
                loading={saveEnv.loading}
                onClick={() => saveEnv.run(undefined)}
              >
                Save
              </Button>
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}
