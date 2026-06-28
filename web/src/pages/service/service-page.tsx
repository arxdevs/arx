import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { serviceApi, type Service } from "@/entities/service";
import { ServiceActions } from "@/features/service-actions";
import { ServiceConfig } from "@/features/service-config";
import { DeploymentsList } from "@/features/deployments-list";
import { LogViewer } from "@/features/log-viewer";
import { BackupsManager } from "@/features/backups-manager";
import { RenameDialog } from "@/features/rename-dialog";
import { DeleteEntityDialog } from "@/features/delete-entity";
import { Breadcrumbs } from "@/widgets/breadcrumbs";
import {
  PageHeader,
  Spinner,
  ErrorMessage,
  StatusBadge,
  Tabs,
  Button,
} from "@/shared/ui";
import { useQuery, useTabParam } from "@/shared/lib";
import { OverviewTab } from "./overview-tab";
import { VariablesTab } from "./variables-tab";
import { DomainsTab } from "./domains-tab";

const TABS = [
  "overview",
  "deployments",
  "logs",
  "variables",
  "domains",
  "config",
  "backups",
];

export function ServicePage() {
  const { ws = "", proj = "", svc = "" } = useParams();
  const navigate = useNavigate();
  const [tab, setTab] = useTabParam("overview");
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const { data, error, loading, reload } = useQuery<Service>(
    () => serviceApi.get(ws, proj, svc),
    [ws, proj, svc],
  );

  return (
    <>
      <Breadcrumbs
        items={[
          { label: "Workspaces", to: "/" },
          { label: ws, to: `/w/${ws}` },
          { label: proj, to: `/w/${ws}/p/${proj}` },
          { label: svc },
        ]}
      />

      {loading && <Spinner label="loading service" />}
      {error && <ErrorMessage message={error.message} />}

      {data && (
        <>
          <PageHeader
            title={data.name}
            subtitle={data.slug}
            actions={
              <>
                <StatusBadge status={data.kind} />
                <ServiceActions
                  ws={ws}
                  proj={proj}
                  svc={svc}
                  onDone={reload}
                />
                <Button size="md" onClick={() => setRenaming(true)}>
                  Rename
                </Button>
                <Button
                  size="md"
                  variant="danger"
                  onClick={() => setDeleting(true)}
                >
                  Delete
                </Button>
              </>
            }
          />

          <Tabs tabs={TABS} active={tab} onChange={setTab} />

          {tab === "overview" && <OverviewTab service={data} />}
          {tab === "deployments" && (
            <DeploymentsList ws={ws} proj={proj} svc={svc} />
          )}
          {tab === "logs" && <LogViewer ws={ws} proj={proj} svc={svc} />}
          {tab === "variables" && (
            <VariablesTab ws={ws} proj={proj} svc={svc} />
          )}
          {tab === "domains" && <DomainsTab ws={ws} proj={proj} svc={svc} />}
          {tab === "config" && (
            <ServiceConfig
              ws={ws}
              proj={proj}
              svc={svc}
              service={data}
              onSaved={reload}
            />
          )}
          {tab === "backups" && (
            <BackupsManager ws={ws} proj={proj} svc={svc} />
          )}

          {renaming && (
            <RenameDialog
              title="Rename service"
              open
              current={data.name}
              onClose={() => setRenaming(false)}
              onRename={(name) => serviceApi.rename(ws, proj, svc, name)}
              onRenamed={reload}
            />
          )}
          {deleting && (
            <DeleteEntityDialog
              title="Delete service"
              entityName={data.name}
              open
              withDataOption
              onClose={() => setDeleting(false)}
              onDelete={(opts) => serviceApi.remove(ws, proj, svc, opts)}
              onDeleted={() => navigate(`/w/${ws}/p/${proj}`)}
            />
          )}
        </>
      )}
    </>
  );
}
