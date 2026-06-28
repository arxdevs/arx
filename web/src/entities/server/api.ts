import { api } from "@/shared/api";

export interface ServerSettings {
  admin_domain: string | null;
  acme_email: string | null;
  public_ip: string | null;
}

export const serverApi = {
  settings: () => api.get<ServerSettings>("/v1/server/settings"),
  updateSettings: (input: Partial<ServerSettings>) =>
    api.patch<ServerSettings>("/v1/server/settings", input),
  certRetry: () => api.post<unknown>("/v1/server/cert/retry"),
  githubSync: () => api.post<unknown>("/v1/server/github/sync"),
};
