import { api } from "@/shared/api";

export interface Variable {
  key: string;
  value: string;
  sealed: boolean;
}

const base = (ws: string, proj: string, svc: string) =>
  `/v1/workspaces/${ws}/projects/${proj}/services/${svc}/variables`;

export const variableApi = {
  list: (ws: string, proj: string, svc: string, env?: string) =>
    api.get<Variable[]>(base(ws, proj, svc), env ? { env } : undefined),
  set: (
    ws: string,
    proj: string,
    svc: string,
    input: { key: string; value: string; sealed: boolean; env?: string },
  ) => api.post<void>(base(ws, proj, svc), input),
  unset: (ws: string, proj: string, svc: string, key: string, env?: string) =>
    api.delete<void>(
      `${base(ws, proj, svc)}/${encodeURIComponent(key)}${
        env ? `?env=${encodeURIComponent(env)}` : ""
      }`,
    ),
};
