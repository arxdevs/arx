import { useState } from "react";
import { volumeApi, type VolumeReport, type PruneResult } from "@/entities/volume";
import {
  Card,
  Button,
  Spinner,
  ErrorMessage,
  EmptyState,
  DataTable,
  StatusBadge,
} from "@/shared/ui";
import { useQuery } from "@/shared/lib";
import styles from "./volumes-manager.module.css";

export function VolumesManager({ ws }: { ws: string }) {
  const volumes = useQuery<VolumeReport[]>(() => volumeApi.list(ws), [ws]);
  const [result, setResult] = useState<PruneResult | null>(null);
  const [pruning, setPruning] = useState(false);
  const [pruneError, setPruneError] = useState<string>();

  const runPrune = async (execute: boolean) => {
    setPruning(true);
    setPruneError(undefined);
    try {
      const r = await volumeApi.prune(ws, execute);
      setResult(r);
      volumes.reload();
    } catch (err) {
      setPruneError(err instanceof Error ? err.message : String(err));
    } finally {
      setPruning(false);
    }
  };

  return (
    <Card>
      <div className={styles.head}>
        <h3 className="arx-section-title">Volumes</h3>
        <div className="arx-row">
          <Button size="sm" onClick={() => void runPrune(false)}>
            Dry-run prune
          </Button>
          <Button
            size="sm"
            variant="danger"
            loading={pruning}
            onClick={() => void runPrune(true)}
          >
            Prune orphans
          </Button>
        </div>
      </div>

      {pruneError && <ErrorMessage message={pruneError} />}
      {result && (
        <div className={styles.result}>
          {result.dry_run ? "Dry-run: " : "Pruned: "}
          {result.removed.length} removed, {result.skipped.length} skipped
        </div>
      )}

      {volumes.loading && <Spinner label="loading volumes" />}
      {volumes.error && <ErrorMessage message={volumes.error.message} />}
      {volumes.data && volumes.data.length === 0 && (
        <EmptyState title="No managed volumes" />
      )}
      {volumes.data && volumes.data.length > 0 && (
        <DataTable
          rowKey={(v) => v.name}
          rows={volumes.data}
          columns={[
            { header: "Name", cell: (v) => <code>{v.name}</code> },
            {
              header: "Status",
              cell: (v) => <StatusBadge status={v.classification} />,
            },
            {
              header: "Service",
              cell: (v) =>
                v.service_id ? (
                  <code>{v.service_id.slice(0, 8)}</code>
                ) : (
                  "—"
                ),
            },
          ]}
        />
      )}
    </Card>
  );
}
