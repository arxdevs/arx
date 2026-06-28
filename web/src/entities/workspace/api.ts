import { api, deleteQuery, type DeleteOptions } from "@/shared/api";

export interface Workspace {
  id: string;
  slug: string;
  name: string;
  role?: string;
}

export const workspaceApi = {
  list: () => api.get<Workspace[]>("/v1/workspaces"),
  get: (ws: string) => api.get<Workspace>(`/v1/workspaces/${ws}`),
  create: (input: { slug: string; name: string }) =>
    api.post<Workspace>("/v1/workspaces", input),
  rename: (ws: string, name: string) =>
    api.patch<Workspace>(`/v1/workspaces/${ws}`, { name }),
  remove: (ws: string, opts?: DeleteOptions) =>
    api.delete<void>(`/v1/workspaces/${ws}${deleteQuery(opts)}`),
};
