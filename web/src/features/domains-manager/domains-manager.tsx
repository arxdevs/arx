import { useState } from "react";
import { domainApi, type ServiceDomain } from "@/entities/domain";
import { Button, Field, ErrorMessage, StatusBadge } from "@/shared/ui";
import { useMutation } from "@/shared/lib";
import styles from "./domains-manager.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
  domains: ServiceDomain[];
  onChange: () => void;
}

export function DomainsManager({
  ws,
  proj,
  svc,
  env,
  domains,
  onChange,
}: Props) {
  const [hostname, setHostname] = useState("");

  const add = useMutation(
    () => domainApi.add(ws, proj, svc, { hostname, env }),
    () => {
      setHostname("");
      onChange();
    },
  );

  const remove = useMutation(
    (id: string) => domainApi.remove(ws, proj, svc, id),
    onChange,
  );

  return (
    <div className={styles.wrap}>
      <ul className={styles.list}>
        {domains.map((d) => (
          <li key={d.id} className={styles.item}>
            <span className={styles.host}>{d.hostname}</span>
            <StatusBadge status={d.cert_status} />
            <Button variant="danger" onClick={() => remove.run(d.id)}>
              Remove
            </Button>
          </li>
        ))}
      </ul>

      <div className={styles.form}>
        <Field
          label="Hostname"
          value={hostname}
          placeholder="app.example.com"
          onChange={(e) => setHostname(e.target.value)}
        />
        <Button
          variant="primary"
          loading={add.loading}
          disabled={!hostname}
          onClick={() => add.run(undefined)}
        >
          Add
        </Button>
      </div>
      {add.error && <ErrorMessage message={add.error.message} />}
    </div>
  );
}
