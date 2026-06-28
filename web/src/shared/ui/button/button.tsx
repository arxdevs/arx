import type { ButtonHTMLAttributes } from "react";
import styles from "./button.module.css";

type Variant = "primary" | "ghost" | "danger" | "subtle";
type Size = "sm" | "md";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
}

export function Button({
  variant = "subtle",
  size = "md",
  loading = false,
  disabled,
  children,
  className,
  ...rest
}: Props) {
  return (
    <button
      className={[styles.button, styles[variant], styles[size], className]
        .filter(Boolean)
        .join(" ")}
      disabled={disabled || loading}
      {...rest}
    >
      {loading && <span className={styles.spinner} />}
      {children}
    </button>
  );
}
