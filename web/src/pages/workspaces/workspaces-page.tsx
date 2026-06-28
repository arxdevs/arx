import { useState } from "react";
import { workspaceApi, type Workspace } from "@/entities/workspace";
import { SlugNameForm } from "@/features/slug-name-form";
import {
  PageHeader,
  Button,
  Spinner,
  ErrorMessage,
  EmptyState,
  EntityCard,
  StatusBadge,
} from "@/shared/ui";
import { useQuery } from "@/shared/lib";

export function WorkspacesPage() {
  const [creating, setCreating] = useState(false);
  const { data, error, loading, reload } = useQuery<Workspace[]>(
    () => workspaceApi.list(),
    [],
  );

  return (
    <>
      <PageHeader
        title="Workspaces"
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New workspace
          </Button>
        }
      />

      {loading && <Spinner label="loading workspaces" />}
      {error && <ErrorMessage message={error.message} />}

      {data && data.length === 0 && (
        <EmptyState
          title="No workspaces yet"
          hint="Create your first workspace to get started."
        />
      )}

      {data && data.length > 0 && (
        <div className="arx-grid">
          {data.map((w) => (
            <EntityCard
              key={w.id}
              to={`/w/${w.slug}`}
              title={w.name}
              subtitle={w.slug}
              badge={w.role ? <StatusBadge status={w.role} /> : undefined}
            />
          ))}
        </div>
      )}

      <SlugNameForm
        title="New workspace"
        open={creating}
        onClose={() => setCreating(false)}
        onCreate={(input) => workspaceApi.create(input)}
        onCreated={reload}
      />
    </>
  );
}
