import { webhookApi, type WebhookDelivery } from "@/entities/webhook";
import {
  Modal,
  Button,
  Spinner,
  ErrorMessage,
  EmptyState,
  DataTable,
  StatusBadge,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";

interface Props {
  ws: string;
  endpointId: string | null;
  onClose: () => void;
}

export function DeliveriesModal({ ws, endpointId, onClose }: Props) {
  const deliveries = useQuery<WebhookDelivery[]>(
    () =>
      endpointId
        ? webhookApi.deliveries(ws, endpointId)
        : Promise.resolve([]),
    [ws, endpointId],
  );

  const redeliver = useMutation(
    (deliveryId: string) =>
      webhookApi.redeliver(ws, endpointId!, deliveryId),
    deliveries.reload,
  );

  return (
    <Modal
      title="Recent deliveries"
      open={endpointId !== null}
      onClose={onClose}
    >
      {deliveries.loading && <Spinner label="loading deliveries" />}
      {deliveries.error && <ErrorMessage message={deliveries.error.message} />}
      {deliveries.data && deliveries.data.length === 0 && (
        <EmptyState title="No deliveries yet" />
      )}
      {deliveries.data && deliveries.data.length > 0 && (
        <DataTable
          rowKey={(d) => d.id}
          rows={deliveries.data}
          columns={[
            { header: "Event", cell: (d) => <code>{d.event_type}</code> },
            { header: "Status", cell: (d) => <StatusBadge status={d.status} /> },
            { header: "Attempts", cell: (d) => d.attempts },
            {
              header: "",
              align: "right",
              cell: (d) => (
                <Button
                  size="sm"
                  loading={redeliver.loading}
                  onClick={() => redeliver.run(d.id)}
                >
                  Redeliver
                </Button>
              ),
            },
          ]}
        />
      )}
    </Modal>
  );
}
