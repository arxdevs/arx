import { api } from "@/shared/api";

export interface Member {
  user_id: string;
  display_name: string;
  github_login: string | null;
  role: string;
}

const base = (ws: string) => `/v1/workspaces/${ws}/members`;

export const memberApi = {
  list: (ws: string) => api.get<Member[]>(base(ws)),
  invite: (ws: string, input: { github_login: string; role: string }) =>
    api.post<unknown>(base(ws), input),
  remove: (ws: string, userId: string) =>
    api.delete<void>(`${base(ws)}/${userId}`),
};
