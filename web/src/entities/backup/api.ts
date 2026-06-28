import { api } from "@/shared/api";

export interface Backup {
  id: string;
  size_bytes: number;
  storage_uri: string;
  created_at: string;
}

export interface BackupSchedule {
  cron_expression: string;
  retention_count: number;
  storage: string;
  enabled: boolean;
}

const base = (ws: string, proj: string, svc: string) =>
  `/v1/workspaces/${ws}/projects/${proj}/services/${svc}`;

export const backupApi = {
  list: (ws: string, proj: string, svc: string) =>
    api.get<Backup[]>(`${base(ws, proj, svc)}/backups`),
  now: (ws: string, proj: string, svc: string) =>
    api.post<Backup>(`${base(ws, proj, svc)}/backups`),
  restore: (ws: string, proj: string, svc: string, storageUri: string) =>
    api.post<void>(`${base(ws, proj, svc)}/backups/restore`, {
      storage_uri: storageUri,
    }),
  getSchedule: (ws: string, proj: string, svc: string) =>
    api.get<BackupSchedule | null>(`${base(ws, proj, svc)}/backup-schedule`),
  putSchedule: (
    ws: string,
    proj: string,
    svc: string,
    input: BackupSchedule,
  ) => api.put<void>(`${base(ws, proj, svc)}/backup-schedule`, input),
};
