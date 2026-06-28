import type { ReactNode } from "react";
import styles from "./empty-state.module.css";

interface Props {
  title: string;
  hint?: string;
  action?: ReactNode;
}

export function EmptyState({ title, hint, action }: Props) {
  return (
    <div className={styles.empty}>
      <p className={styles.title}>{title}</p>
      {hint && <p className={styles.hint}>{hint}</p>}
      {action}
    </div>
  );
}
