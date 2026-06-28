import { Link } from "react-router-dom";
import { Fragment } from "react";
import styles from "./breadcrumbs.module.css";

export interface Crumb {
  label: string;
  to?: string;
}

export function Breadcrumbs({ items }: { items: Crumb[] }) {
  return (
    <nav className={styles.crumbs}>
      {items.map((item, i) => (
        <Fragment key={i}>
          {i > 0 && <span className={styles.sep}>/</span>}
          {item.to ? (
            <Link to={item.to}>{item.label}</Link>
          ) : (
            <span className={styles.current}>{item.label}</span>
          )}
        </Fragment>
      ))}
    </nav>
  );
}
