import styles from "./spinner.module.css";

export function Spinner({ label }: { label?: string }) {
  return (
    <div className={styles.wrap}>
      <span className={styles.dot} />
      {label && <span className={styles.label}>{label}</span>}
    </div>
  );
}
