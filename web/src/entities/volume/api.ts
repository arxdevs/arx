import { api } from "@/shared/api";

export interface VolumeReport {
  name: string;
  service_id: string | null;
  environment_id: string | null;
  classification: string;
}

export interface PruneResult {
  removed: string[];
  skipped: Array<{ name: string; reason: string }>;
  dry_run: boolean;
}

const base = (ws: string) => `/v1/workspaces/${ws}/admin/volumes`;

export const volumeApi = {
  list: (ws: string) => api.get<VolumeReport[]>(base(ws)),
  prune: (ws: string, execute: boolean) =>
    api.post<PruneResult>(
      `${base(ws)}/prune${execute ? "?execute=true" : ""}`,
    ),
};
