import type { SelectHTMLAttributes } from "react";
import styles from "./select.module.css";

interface Props extends SelectHTMLAttributes<HTMLSelectElement> {
  label?: string;
  options: Array<{ value: string; label: string }>;
}

export function Select({ label, options, id, ...rest }: Props) {
  const selectId = id ?? label?.toLowerCase().replace(/\s+/g, "-");
  return (
    <label className={styles.field} htmlFor={selectId}>
      {label && <span className={styles.label}>{label}</span>}
      <select id={selectId} className={styles.select} {...rest}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}
