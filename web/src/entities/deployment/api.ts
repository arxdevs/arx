import { api } from "@/shared/api";

export interface Deployment {
  id: string;
  status: string;
  image_ref: string | null;
  commit_sha: string | null;
  container_id: string | null;
  error: string | null;
  created_at: string;
}

export const deploymentApi = {
  list: (ws: string, proj: string, svc: string, env?: string) =>
    api.get<Deployment[]>(
      `/v1/workspaces/${ws}/projects/${proj}/services/${svc}/deployments`,
      env ? { env } : undefined,
    ),
};
