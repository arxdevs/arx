import { api, deleteQuery, type DeleteOptions } from "@/shared/api";

export interface ServiceSource {
  kind?: string;
  github_repo?: string;
  branch?: string;
  image?: string;
  template?: string;
  root_directory?: string;
  dockerfile?: string | null;
  [key: string]: unknown;
}

export interface Service {
  id: string;
  slug: string;
  name: string;
  kind: string;
  source: ServiceSource;
  build_command: string | null;
  start_command: string | null;
  pre_deploy_command: string | null;
  restart_policy: string;
}

export interface EnvConfig {
  cpu_limit: number | null;
  memory_limit_mb: number | null;
  healthcheck_mode: string;
  healthcheck_path: string | null;
  healthcheck_timeout_seconds: number;
}

export interface CreateServiceInput {
  slug: string;
  name: string;
  kind: "git" | "image" | "db";
  repo?: string;
  branch?: string;
  image?: string;
  template?: string;
}

export interface ServiceConfigPatch {
  name?: string;
  build_command?: string | null;
  start_command?: string | null;
  pre_deploy_command?: string | null;
  restart_policy?: string;
}

const base = (ws: string, proj: string) =>
  `/v1/workspaces/${ws}/projects/${proj}/services`;

export const serviceApi = {
  list: (ws: string, proj: string) => api.get<Service[]>(base(ws, proj)),
  get: (ws: string, proj: string, svc: string) =>
    api.get<Service>(`${base(ws, proj)}/${svc}`),
  create: (ws: string, proj: string, input: CreateServiceInput) =>
    api.post<Service>(base(ws, proj), input),
  patch: (ws: string, proj: string, svc: string, input: ServiceConfigPatch) =>
    api.patch<Service>(`${base(ws, proj)}/${svc}`, input),
  rename: (ws: string, proj: string, svc: string, name: string) =>
    api.patch<Service>(`${base(ws, proj)}/${svc}`, { name }),
  remove: (ws: string, proj: string, svc: string, opts?: DeleteOptions) =>
    api.delete<void>(`${base(ws, proj)}/${svc}${deleteQuery(opts)}`),
  deploy: (ws: string, proj: string, svc: string, env?: string) =>
    api.post<unknown>(`${base(ws, proj)}/${svc}/deploy`, { env }),
  restart: (ws: string, proj: string, svc: string, env?: string) =>
    api.post<unknown>(`${base(ws, proj)}/${svc}/restart`, { env }),
  rollback: (
    ws: string,
    proj: string,
    svc: string,
    deploymentId: string,
    env?: string,
  ) =>
    api.post<unknown>(`${base(ws, proj)}/${svc}/rollback`, {
      deployment_id: deploymentId,
      env,
    }),
  getConfig: (ws: string, proj: string, svc: string, env?: string) =>
    api.get<EnvConfig>(
      `${base(ws, proj)}/${svc}/config`,
      env ? { env } : undefined,
    ),
  patchConfig: (
    ws: string,
    proj: string,
    svc: string,
    input: Partial<EnvConfig> & { env?: string },
  ) => api.patch<EnvConfig>(`${base(ws, proj)}/${svc}/config`, input),
};
