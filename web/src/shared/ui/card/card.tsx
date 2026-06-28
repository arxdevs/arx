import type { ReactNode } from "react";
import styles from "./card.module.css";

interface Props {
  children: ReactNode;
  className?: string;
}

export function Card({ children, className }: Props) {
  return (
    <div className={[styles.card, className].filter(Boolean).join(" ")}>
      {children}
    </div>
  );
}
