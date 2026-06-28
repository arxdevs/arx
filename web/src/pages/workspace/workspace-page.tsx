import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { workspaceApi } from "@/entities/workspace";
import { projectApi, type Project } from "@/entities/project";
import { SlugNameForm } from "@/features/slug-name-form";
import { RenameDialog } from "@/features/rename-dialog";
import { DeleteEntityDialog } from "@/features/delete-entity";
import { MembersManager } from "@/features/members-manager";
import { WebhooksManager } from "@/features/webhooks-manager";
import { VolumesManager } from "@/features/volumes-manager";
import { Breadcrumbs } from "@/widgets/breadcrumbs";
import {
  PageHeader,
  Button,
  Spinner,
  ErrorMessage,
  EmptyState,
  EntityCard,
  Tabs,
} from "@/shared/ui";
import { useQuery, useTabParam } from "@/shared/lib";

const TABS = ["projects", "members", "webhooks", "volumes"];

export function WorkspacePage() {
  const { ws = "" } = useParams();
  const navigate = useNavigate();
  const [tab, setTab] = useTabParam("projects");
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const projects = useQuery<Project[]>(() => projectApi.list(ws), [ws]);

  return (
    <>
      <Breadcrumbs items={[{ label: "Workspaces", to: "/" }, { label: ws }]} />
      <PageHeader
        title={ws}
        actions={
          <>
            {tab === "projects" && (
              <Button variant="primary" onClick={() => setCreating(true)}>
                New project
              </Button>
            )}
            <Button onClick={() => setRenaming(true)}>Rename</Button>
            <Button variant="danger" onClick={() => setDeleting(true)}>
              Delete
            </Button>
          </>
        }
      />

      <Tabs tabs={TABS} active={tab} onChange={setTab} />

      {tab === "projects" && (
        <>
          {projects.loading && <Spinner label="loading projects" />}
          {projects.error && <ErrorMessage message={projects.error.message} />}
          {projects.data && projects.data.length === 0 && (
            <EmptyState
              title="No projects yet"
              hint="A project groups related services."
            />
          )}
          {projects.data && projects.data.length > 0 && (
            <div className="arx-grid">
              {projects.data.map((p) => (
                <EntityCard
                  key={p.id}
                  to={`/w/${ws}/p/${p.slug}`}
                  title={p.name}
                  subtitle={p.slug}
                />
              ))}
            </div>
          )}
        </>
      )}

      {tab === "members" && <MembersManager ws={ws} />}
      {tab === "webhooks" && <WebhooksManager ws={ws} />}
      {tab === "volumes" && <VolumesManager ws={ws} />}

      <SlugNameForm
        title="New project"
        open={creating}
        onClose={() => setCreating(false)}
        onCreate={(input) => projectApi.create(ws, input)}
        onCreated={projects.reload}
      />
      {renaming && (
        <RenameDialog
          title="Rename workspace"
          open
          current={ws}
          onClose={() => setRenaming(false)}
          onRename={(name) => workspaceApi.rename(ws, name)}
          onRenamed={projects.reload}
        />
      )}
      {deleting && (
        <DeleteEntityDialog
          title="Delete workspace"
          entityName={ws}
          open
          forceOption
          withDataOption
          onClose={() => setDeleting(false)}
          onDelete={(opts) => workspaceApi.remove(ws, opts)}
          onDeleted={() => navigate("/")}
        />
      )}
    </>
  );
}
