import { useState } from "react";
import { environmentApi, type Environment } from "@/entities/environment";
import { SlugNameForm } from "@/features/slug-name-form";
import { RenameDialog } from "@/features/rename-dialog";
import { DeleteEntityDialog } from "@/features/delete-entity";
import {
  Card,
  Button,
  Spinner,
  ErrorMessage,
  DataTable,
  StatusBadge,
} from "@/shared/ui";
import { useQuery } from "@/shared/lib";
import styles from "./environments-manager.module.css";

interface Props {
  ws: string;
  proj: string;
}

export function EnvironmentsManager({ ws, proj }: Props) {
  const envs = useQuery<Environment[]>(
    () => environmentApi.list(ws, proj),
    [ws, proj],
  );
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<Environment | null>(null);
  const [deleting, setDeleting] = useState<Environment | null>(null);

  return (
    <Card>
      <div className={styles.head}>
        <h3 className="arx-section-title">Environments</h3>
        <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
          New environment
        </Button>
      </div>

      {envs.loading && <Spinner label="loading environments" />}
      {envs.error && <ErrorMessage message={envs.error.message} />}
      {envs.data && (
        <DataTable
          rowKey={(e) => e.id}
          rows={envs.data}
          columns={[
            { header: "Name", cell: (e) => e.name },
            { header: "Slug", cell: (e) => <code>{e.slug}</code> },
            {
              header: "Default",
              cell: (e) =>
                e.is_default ? <StatusBadge status="default" /> : "—",
            },
            {
              header: "",
              align: "right",
              cell: (e) => (
                <div className={styles.actions}>
                  <Button size="sm" onClick={() => setRenaming(e)}>
                    Rename
                  </Button>
                  {!e.is_default && (
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() => setDeleting(e)}
                    >
                      Delete
                    </Button>
                  )}
                </div>
              ),
            },
          ]}
        />
      )}

      <SlugNameForm
        title="New environment"
        open={creating}
        onClose={() => setCreating(false)}
        onCreate={(input) => environmentApi.create(ws, proj, input)}
        onCreated={envs.reload}
      />
      {renaming && (
        <RenameDialog
          title="Rename environment"
          open
          current={renaming.name}
          onClose={() => setRenaming(null)}
          onRename={(name) =>
            environmentApi.rename(ws, proj, renaming.slug, name)
          }
          onRenamed={envs.reload}
        />
      )}
      {deleting && (
        <DeleteEntityDialog
          title="Delete environment"
          entityName={deleting.name}
          open
          forceOption
          withDataOption
          onClose={() => setDeleting(null)}
          onDelete={(opts) =>
            environmentApi.remove(ws, proj, deleting.slug, opts)
          }
          onDeleted={envs.reload}
        />
      )}
    </Card>
  );
}
