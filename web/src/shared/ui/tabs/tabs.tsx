import styles from "./tabs.module.css";

interface Props {
  tabs: string[];
  active: string;
  onChange: (tab: string) => void;
}

export function Tabs({ tabs, active, onChange }: Props) {
  return (
    <div className={styles.tabs}>
      {tabs.map((tab) => (
        <button
          key={tab}
          type="button"
          className={`${styles.tab} ${active === tab ? styles.active : ""}`}
          onClick={() => onChange(tab)}
        >
          {tab}
        </button>
      ))}
    </div>
  );
}
