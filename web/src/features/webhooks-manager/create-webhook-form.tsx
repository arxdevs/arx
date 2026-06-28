import { useState } from "react";
import { webhookApi } from "@/entities/webhook";
import {
  Modal,
  Field,
  Button,
  ErrorMessage,
} from "@/shared/ui";
import { useMutation } from "@/shared/lib";
import styles from "./webhooks-manager.module.css";

interface Props {
  ws: string;
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}

export function CreateWebhookForm({ ws, open, onClose, onCreated }: Props) {
  const [url, setUrl] = useState("");
  const [events, setEvents] = useState("");
  const [secret, setSecret] = useState<string | null>(null);

  const create = useMutation(
    async () => {
      const res = await webhookApi.create(ws, {
        url,
        events: events
          ? events.split(",").map((e) => e.trim()).filter(Boolean)
          : undefined,
      });
      setSecret(res.secret);
    },
    () => {
      setUrl("");
      setEvents("");
      onCreated();
    },
  );

  const close = () => {
    setSecret(null);
    onClose();
  };

  return (
    <Modal
      title="New webhook endpoint"
      open={open}
      onClose={close}
      footer={
        secret ? (
          <Button variant="primary" onClick={close}>
            Done
          </Button>
        ) : (
          <>
            <Button onClick={close}>Cancel</Button>
            <Button
              variant="primary"
              loading={create.loading}
              disabled={!url}
              onClick={() => create.run(undefined)}
            >
              Create
            </Button>
          </>
        )
      }
    >
      {secret ? (
        <div className={styles.secret}>
          <p>Copy the signing secret now — it is shown only once:</p>
          <code className={styles.secretValue}>{secret}</code>
        </div>
      ) : (
        <>
          <Field
            label="URL"
            value={url}
            placeholder="https://example.com/hook"
            onChange={(e) => setUrl(e.target.value)}
          />
          <Field
            label="Events (comma-separated, blank = all)"
            value={events}
            placeholder="deployment.succeeded, backup.failed"
            onChange={(e) => setEvents(e.target.value)}
          />
          {create.error && <ErrorMessage message={create.error.message} />}
        </>
      )}
    </Modal>
  );
}
