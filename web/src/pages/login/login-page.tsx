import { LoginButton } from "@/features/auth";
import { Card } from "@/shared/ui";
import styles from "./login-page.module.css";

export function LoginPage() {
  return (
    <div className={styles.wrap}>
      <Card className={styles.card}>
        <h1 className={styles.brand}>arx</h1>
        <p className={styles.sub}>Self-hosted deploys, from your browser.</p>
        <LoginButton />
      </Card>
    </div>
  );
}
