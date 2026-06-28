import { deploymentApi, type Deployment } from "@/entities/deployment";
import { serviceApi } from "@/entities/service";
import { StatusBadge, Button, Spinner, ErrorMessage, EmptyState } from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import styles from "./deployments-list.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
}

function shortId(id: string) {
  return id.slice(0, 8);
}

export function DeploymentsList({ ws, proj, svc, env }: Props) {
  const { data, error, loading, reload } = useQuery<Deployment[]>(
    () => deploymentApi.list(ws, proj, svc, env),
    [ws, proj, svc, env],
  );

  const rollback = useMutation(
    (id: string) => serviceApi.rollback(ws, proj, svc, id, env),
    reload,
  );

  if (loading) return <Spinner label="loading deployments" />;
  if (error) return <ErrorMessage message={error.message} />;
  if (!data || data.length === 0)
    return <EmptyState title="No deployments yet" />;

  return (
    <div>
      {rollback.error && <ErrorMessage message={rollback.error.message} />}
      <table className={styles.table}>
        <thead>
          <tr>
            <th>ID</th>
            <th>Status</th>
            <th>Commit</th>
            <th>Created</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {data.map((d) => (
            <tr key={d.id}>
              <td className={styles.mono}>{shortId(d.id)}</td>
              <td>
                <StatusBadge status={d.status} />
              </td>
              <td className={styles.mono}>
                {d.commit_sha ? d.commit_sha.slice(0, 7) : "—"}
              </td>
              <td className={styles.muted}>
                {new Date(d.created_at).toLocaleString()}
              </td>
              <td className={styles.right}>
                <Button
                  loading={rollback.loading}
                  onClick={() => rollback.run(d.id)}
                >
                  Rollback
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
