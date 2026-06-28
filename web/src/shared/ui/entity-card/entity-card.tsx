import { Link } from "react-router-dom";
import type { ReactNode } from "react";
import styles from "./entity-card.module.css";

interface Props {
  to: string;
  title: string;
  subtitle?: string;
  badge?: ReactNode;
}

export function EntityCard({ to, title, subtitle, badge }: Props) {
  return (
    <Link to={to} className={styles.card}>
      <div className={styles.head}>
        <span className={styles.title}>{title}</span>
        {badge}
      </div>
      {subtitle && <span className={styles.subtitle}>{subtitle}</span>}
    </Link>
  );
}
