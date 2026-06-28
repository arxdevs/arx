import { useState } from "react";
import { variableApi, type Variable } from "@/entities/variable";
import { Button, Field, ErrorMessage } from "@/shared/ui";
import { useMutation } from "@/shared/lib";
import styles from "./variables-manager.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
  variables: Variable[];
  onChange: () => void;
}

export function VariablesManager({
  ws,
  proj,
  svc,
  env,
  variables,
  onChange,
}: Props) {
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [sealed, setSealed] = useState(false);

  const set = useMutation(
    () => variableApi.set(ws, proj, svc, { key, value, sealed, env }),
    () => {
      setKey("");
      setValue("");
      setSealed(false);
      onChange();
    },
  );

  const unset = useMutation(
    (k: string) => variableApi.unset(ws, proj, svc, k, env),
    onChange,
  );

  return (
    <div className={styles.wrap}>
      <table className={styles.table}>
        <tbody>
          {variables.map((v) => (
            <tr key={v.key}>
              <td className={styles.key}>{v.key}</td>
              <td className={styles.value}>
                {v.sealed ? <span className={styles.sealed}>sealed</span> : v.value}
              </td>
              <td className={styles.actions}>
                <Button variant="danger" onClick={() => unset.run(v.key)}>
                  Remove
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <div className={styles.form}>
        <Field
          label="Key"
          value={key}
          placeholder="DATABASE_URL"
          onChange={(e) => setKey(e.target.value)}
        />
        <Field
          label="Value"
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <label className={styles.sealedToggle}>
          <input
            type="checkbox"
            checked={sealed}
            onChange={(e) => setSealed(e.target.checked)}
          />
          Sealed
        </label>
        <Button
          variant="primary"
          loading={set.loading}
          disabled={!key}
          onClick={() => set.run(undefined)}
        >
          Set
        </Button>
      </div>
      {set.error && <ErrorMessage message={set.error.message} />}
    </div>
  );
}
