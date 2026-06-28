import { api } from "@/shared/api";

export interface WebhookEndpoint {
  id: string;
  workspace_id: string;
  project_id: string | null;
  kind: string;
  url: string;
  events: string[];
  active: boolean;
  consecutive_failures: number;
  disabled_reason: string | null;
  created_at: string;
  updated_at: string;
}

export interface WebhookDelivery {
  id: string;
  event_id: string;
  event_type: string;
  status: string;
  attempts: number;
  response_status: number | null;
  error: string | null;
  created_at: string;
  delivered_at: string | null;
  exhausted_at: string | null;
}

export interface CreateWebhookInput {
  url: string;
  events?: string[];
  project?: string;
  secret?: string;
}

const base = (ws: string) => `/v1/workspaces/${ws}/webhooks`;

export const webhookApi = {
  list: (ws: string) => api.get<WebhookEndpoint[]>(base(ws)),
  get: (ws: string, id: string) =>
    api.get<WebhookEndpoint>(`${base(ws)}/${id}`),
  create: (ws: string, input: CreateWebhookInput) =>
    api.post<WebhookEndpoint & { secret: string }>(base(ws), input),
  patch: (
    ws: string,
    id: string,
    input: { url?: string; events?: string[]; active?: boolean },
  ) => api.patch<WebhookEndpoint>(`${base(ws)}/${id}`, input),
  remove: (ws: string, id: string) => api.delete<void>(`${base(ws)}/${id}`),
  enable: (ws: string, id: string) =>
    api.post<unknown>(`${base(ws)}/${id}/enable`),
  test: (ws: string, id: string) =>
    api.post<{ delivery_id: string | null }>(`${base(ws)}/${id}/test`),
  deliveries: (ws: string, id: string) =>
    api.get<WebhookDelivery[]>(`${base(ws)}/${id}/deliveries`),
  redeliver: (ws: string, id: string, deliveryId: string) =>
    api.post<unknown>(`${base(ws)}/${id}/deliveries/${deliveryId}/redeliver`),
};
