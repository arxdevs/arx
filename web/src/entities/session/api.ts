import { api } from "@/shared/api";

export interface CurrentUser {
  id: string;
  display_name: string;
  github_login: string | null;
}

export const sessionApi = {
  me: () => api.get<CurrentUser>("/v1/auth/me"),
  logout: () => api.post<void>("/v1/auth/logout"),
};
