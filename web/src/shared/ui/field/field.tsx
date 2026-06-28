import type { InputHTMLAttributes } from "react";
import styles from "./field.module.css";

interface Props extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
}

export function Field({ label, id, ...rest }: Props) {
  const fieldId = id ?? label.toLowerCase().replace(/\s+/g, "-");
  return (
    <label className={styles.field} htmlFor={fieldId}>
      <span className={styles.label}>{label}</span>
      <input id={fieldId} className={styles.input} {...rest} />
    </label>
  );
}
