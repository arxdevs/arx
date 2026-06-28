import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { projectApi } from "@/entities/project";
import { serviceApi, type Service } from "@/entities/service";
import { CreateServiceForm } from "@/features/create-service-form";
import { EnvironmentsManager } from "@/features/environments-manager";
import { RenameDialog } from "@/features/rename-dialog";
import { DeleteEntityDialog } from "@/features/delete-entity";
import { Breadcrumbs } from "@/widgets/breadcrumbs";
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

export function ProjectPage() {
  const { ws = "", proj = "" } = useParams();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const services = useQuery<Service[]>(
    () => serviceApi.list(ws, proj),
    [ws, proj],
  );

  return (
    <>
      <Breadcrumbs
        items={[
          { label: "Workspaces", to: "/" },
          { label: ws, to: `/w/${ws}` },
          { label: proj },
        ]}
      />
      <PageHeader
        title={proj}
        actions={
          <>
            <Button variant="primary" onClick={() => setCreating(true)}>
              New service
            </Button>
            <Button onClick={() => setRenaming(true)}>Rename</Button>
            <Button variant="danger" onClick={() => setDeleting(true)}>
              Delete
            </Button>
          </>
        }
      />

      <div className="arx-stack">
        <section>
          <h2 className="arx-section-title">Services</h2>
          {services.loading && <Spinner label="loading services" />}
          {services.error && <ErrorMessage message={services.error.message} />}
          {services.data && services.data.length === 0 && (
            <EmptyState
              title="No services yet"
              hint="Add a Git repo, image, or database."
            />
          )}
          {services.data && services.data.length > 0 && (
            <div className="arx-grid">
              {services.data.map((s) => (
                <EntityCard
                  key={s.id}
                  to={`/w/${ws}/p/${proj}/s/${s.slug}`}
                  title={s.name}
                  subtitle={s.slug}
                  badge={<StatusBadge status={s.kind} />}
                />
              ))}
            </div>
          )}
        </section>

        <EnvironmentsManager ws={ws} proj={proj} />
      </div>

      <CreateServiceForm
        ws={ws}
        proj={proj}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={services.reload}
      />
      {renaming && (
        <RenameDialog
          title="Rename project"
          open
          current={proj}
          onClose={() => setRenaming(false)}
          onRename={(name) => projectApi.rename(ws, proj, name)}
          onRenamed={services.reload}
        />
      )}
      {deleting && (
        <DeleteEntityDialog
          title="Delete project"
          entityName={proj}
          open
          forceOption
          withDataOption
          onClose={() => setDeleting(false)}
          onDelete={(opts) => projectApi.remove(ws, proj, opts)}
          onDeleted={() => navigate(`/w/${ws}`)}
        />
      )}
    </>
  );
}
