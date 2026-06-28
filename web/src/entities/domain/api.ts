import { api } from "@/shared/api";

export interface ServiceDomain {
  id: string;
  hostname: string;
  verified: boolean;
  cert_status: string;
}

const base = (ws: string, proj: string, svc: string) =>
  `/v1/workspaces/${ws}/projects/${proj}/services/${svc}/domains`;

export const domainApi = {
  list: (ws: string, proj: string, svc: string, env?: string) =>
    api.get<ServiceDomain[]>(base(ws, proj, svc), env ? { env } : undefined),
  add: (
    ws: string,
    proj: string,
    svc: string,
    input: { hostname: string; env?: string },
  ) => api.post<ServiceDomain>(base(ws, proj, svc), input),
  remove: (ws: string, proj: string, svc: string, id: string) =>
    api.delete<void>(`${base(ws, proj, svc)}/${id}`),
};
