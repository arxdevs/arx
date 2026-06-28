import type { ReactNode } from "react";
import { Button } from "@/shared/ui/button/button";
import styles from "./modal.module.css";

interface Props {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}

export function Modal({ title, open, onClose, children, footer }: Props) {
  if (!open) return null;
  return (
    <div className={styles.backdrop} onClick={onClose}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        <header className={styles.header}>
          <h3>{title}</h3>
          <Button variant="ghost" onClick={onClose} aria-label="close">
            ✕
          </Button>
        </header>
        <div className={styles.body}>{children}</div>
        {footer && <footer className={styles.footer}>{footer}</footer>}
      </div>
    </div>
  );
}
