import { api, deleteQuery, type DeleteOptions } from "@/shared/api";

export interface Environment {
  id: string;
  slug: string;
  name: string;
  is_default: boolean;
}

const base = (ws: string, proj: string) =>
  `/v1/workspaces/${ws}/projects/${proj}/environments`;

export const environmentApi = {
  list: (ws: string, proj: string) => api.get<Environment[]>(base(ws, proj)),
  create: (ws: string, proj: string, input: { slug: string; name: string }) =>
    api.post<Environment>(base(ws, proj), input),
  rename: (ws: string, proj: string, env: string, name: string) =>
    api.patch<Environment>(`${base(ws, proj)}/${env}`, { name }),
  remove: (ws: string, proj: string, env: string, opts?: DeleteOptions) =>
    api.delete<void>(`${base(ws, proj)}/${env}${deleteQuery(opts)}`),
};
