import { useEffect, useState } from "react";
import {
  backupApi,
  type Backup,
  type BackupSchedule,
} from "@/entities/backup";
import {
  Card,
  Button,
  Field,
  Select,
  Checkbox,
  Spinner,
  ErrorMessage,
  EmptyState,
  DataTable,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import styles from "./backups-manager.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

export function BackupsManager({ ws, proj, svc }: Props) {
  const backups = useQuery<Backup[]>(
    () => backupApi.list(ws, proj, svc),
    [ws, proj, svc],
  );
  const schedule = useQuery<BackupSchedule | null>(
    () => backupApi.getSchedule(ws, proj, svc),
    [ws, proj, svc],
  );

  const [cron, setCron] = useState("0 3 * * *");
  const [retention, setRetention] = useState("7");
  const [storage, setStorage] = useState("local");
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    if (schedule.data) {
      setCron(schedule.data.cron_expression);
      setRetention(schedule.data.retention_count.toString());
      setStorage(schedule.data.storage);
      setEnabled(schedule.data.enabled);
    }
  }, [schedule.data]);

  const backupNow = useMutation(
    () => backupApi.now(ws, proj, svc),
    backups.reload,
  );
  const restore = useMutation((uri: string) =>
    backupApi.restore(ws, proj, svc, uri),
  );
  const saveSchedule = useMutation(
    () =>
      backupApi.putSchedule(ws, proj, svc, {
        cron_expression: cron,
        retention_count: Number(retention),
        storage,
        enabled,
      }),
    schedule.reload,
  );

  return (
    <div className="arx-stack">
      <Card>
        <div className={styles.head}>
          <h3 className="arx-section-title">Backups</h3>
          <Button
            variant="primary"
            size="sm"
            loading={backupNow.loading}
            onClick={() => backupNow.run(undefined)}
          >
            Back up now
          </Button>
        </div>
        {backupNow.error && <ErrorMessage message={backupNow.error.message} />}
        {restore.error && <ErrorMessage message={restore.error.message} />}
        {backups.loading && <Spinner label="loading backups" />}
        {backups.error && <ErrorMessage message={backups.error.message} />}
        {backups.data && backups.data.length === 0 && (
          <EmptyState title="No backups yet" />
        )}
        {backups.data && backups.data.length > 0 && (
          <DataTable
            rowKey={(b) => b.id}
            rows={backups.data}
            columns={[
              {
                header: "Created",
                cell: (b) => new Date(b.created_at).toLocaleString(),
              },
              { header: "Size", cell: (b) => formatSize(b.size_bytes) },
              {
                header: "Storage",
                cell: (b) => <code>{b.storage_uri}</code>,
              },
              {
                header: "",
                align: "right",
                cell: (b) => (
                  <Button
                    size="sm"
                    loading={restore.loading}
                    onClick={() => restore.run(b.storage_uri)}
                  >
                    Restore
                  </Button>
                ),
              },
            ]}
          />
        )}
      </Card>

      <Card>
        <h3 className="arx-section-title">Schedule</h3>
        {schedule.loading && <Spinner label="loading schedule" />}
        <div className={styles.form}>
          <Field
            label="Cron expression"
            value={cron}
            onChange={(e) => setCron(e.target.value)}
          />
          <div className={styles.grid2}>
            <Field
              label="Retention count"
              value={retention}
              onChange={(e) => setRetention(e.target.value)}
            />
            <Select
              label="Storage"
              options={[{ value: "local", label: "local" }]}
              value={storage}
              onChange={(e) => setStorage(e.target.value)}
            />
          </div>
          <Checkbox
            label="Enabled"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
          />
          {saveSchedule.error && (
            <ErrorMessage message={saveSchedule.error.message} />
          )}
          <div className={styles.actions}>
            <Button
              variant="primary"
              loading={saveSchedule.loading}
              onClick={() => saveSchedule.run(undefined)}
            >
              Save schedule
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}
