import { useState } from "react";
import { webhookApi, type WebhookEndpoint } from "@/entities/webhook";
import {
  Card,
  Button,
  Spinner,
  ErrorMessage,
  EmptyState,
  DataTable,
  StatusBadge,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import { CreateWebhookForm } from "./create-webhook-form";
import { DeliveriesModal } from "./deliveries-modal";
import styles from "./webhooks-manager.module.css";

export function WebhooksManager({ ws }: { ws: string }) {
  const webhooks = useQuery<WebhookEndpoint[]>(
    () => webhookApi.list(ws),
    [ws],
  );
  const [creating, setCreating] = useState(false);
  const [deliveriesFor, setDeliveriesFor] = useState<string | null>(null);

  const toggle = useMutation(
    (ep: WebhookEndpoint) =>
      webhookApi.patch(ws, ep.id, { active: !ep.active }),
    webhooks.reload,
  );
  const enable = useMutation(
    (id: string) => webhookApi.enable(ws, id),
    webhooks.reload,
  );
  const test = useMutation((id: string) => webhookApi.test(ws, id));
  const remove = useMutation(
    (id: string) => webhookApi.remove(ws, id),
    webhooks.reload,
  );

  return (
    <Card>
      <div className={styles.head}>
        <h3 className="arx-section-title">Outgoing webhooks</h3>
        <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
          New webhook
        </Button>
      </div>

      {test.error && <ErrorMessage message={test.error.message} />}
      {webhooks.loading && <Spinner label="loading webhooks" />}
      {webhooks.error && <ErrorMessage message={webhooks.error.message} />}
      {webhooks.data && webhooks.data.length === 0 && (
        <EmptyState title="No webhook endpoints" />
      )}
      {webhooks.data && webhooks.data.length > 0 && (
        <DataTable
          rowKey={(w) => w.id}
          rows={webhooks.data}
          columns={[
            { header: "URL", cell: (w) => <code>{w.url}</code> },
            {
              header: "Events",
              cell: (w) => w.events.join(", ") || "*",
            },
            {
              header: "Status",
              cell: (w) => (
                <StatusBadge status={w.active ? "active" : "inactive"} />
              ),
            },
            {
              header: "",
              align: "right",
              cell: (w) => (
                <div className={styles.rowActions}>
                  <Button
                    size="sm"
                    loading={test.loading}
                    onClick={() => test.run(w.id)}
                  >
                    Test
                  </Button>
                  {w.disabled_reason ? (
                    <Button
                      size="sm"
                      loading={enable.loading}
                      onClick={() => enable.run(w.id)}
                    >
                      Enable
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      loading={toggle.loading}
                      onClick={() => toggle.run(w)}
                    >
                      {w.active ? "Pause" : "Resume"}
                    </Button>
                  )}
                  <Button size="sm" onClick={() => setDeliveriesFor(w.id)}>
                    Deliveries
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    loading={remove.loading}
                    onClick={() => remove.run(w.id)}
                  >
                    Delete
                  </Button>
                </div>
              ),
            },
          ]}
        />
      )}

      <CreateWebhookForm
        ws={ws}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={webhooks.reload}
      />
      <DeliveriesModal
        ws={ws}
        endpointId={deliveriesFor}
        onClose={() => setDeliveriesFor(null)}
      />
    </Card>
  );
}
