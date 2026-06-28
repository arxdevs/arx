import type { InputHTMLAttributes } from "react";
import styles from "./checkbox.module.css";

interface Props extends Omit<InputHTMLAttributes<HTMLInputElement>, "type"> {
  label: string;
}

export function Checkbox({ label, ...rest }: Props) {
  return (
    <label className={styles.wrap}>
      <input type="checkbox" className={styles.input} {...rest} />
      <span>{label}</span>
    </label>
  );
}
