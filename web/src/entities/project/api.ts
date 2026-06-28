import { api, deleteQuery, type DeleteOptions } from "@/shared/api";

export interface Project {
  id: string;
  slug: string;
  name: string;
}

export const projectApi = {
  list: (ws: string) => api.get<Project[]>(`/v1/workspaces/${ws}/projects`),
  get: (ws: string, proj: string) =>
    api.get<Project>(`/v1/workspaces/${ws}/projects/${proj}`),
  create: (ws: string, input: { slug: string; name: string }) =>
    api.post<Project>(`/v1/workspaces/${ws}/projects`, input),
  rename: (ws: string, proj: string, name: string) =>
    api.patch<Project>(`/v1/workspaces/${ws}/projects/${proj}`, { name }),
  remove: (ws: string, proj: string, opts?: DeleteOptions) =>
    api.delete<void>(`/v1/workspaces/${ws}/projects/${proj}${deleteQuery(opts)}`),
};
