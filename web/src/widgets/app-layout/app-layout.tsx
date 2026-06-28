import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { useAuth } from "@/features/auth";
import { Button } from "@/shared/ui";
import styles from "./app-layout.module.css";

function initials(name: string): string {
  return name
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("");
}

export function AppLayout({ children }: { children: ReactNode }) {
  const { user, logout } = useAuth();

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <Link to="/" className={styles.brand}>
          arx
        </Link>
        <div className={styles.right}>
          <Link to="/settings" className={styles.navlink}>
            Settings
          </Link>
          <span className={styles.divider} />
          {user && (
            <span className={styles.user}>
              <span className={styles.avatar}>
                {initials(user.display_name)}
              </span>
              {user.display_name}
            </span>
          )}
          <Button variant="ghost" size="sm" onClick={() => void logout()}>
            Sign out
          </Button>
        </div>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
