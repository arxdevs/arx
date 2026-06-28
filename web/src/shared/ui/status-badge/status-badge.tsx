import styles from "./status-badge.module.css";

type Tone = "ok" | "warn" | "danger" | "muted" | "accent";

const TONE_BY_STATUS: Record<string, Tone> = {
  live: "ok",
  running: "ok",
  succeeded: "ok",
  success: "ok",
  active: "ok",
  ready: "ok",
  inuse: "ok",
  default: "accent",
  admin: "accent",
  git_source: "accent",
  docker_image: "accent",
  db_template: "accent",
  member: "muted",
  pending: "warn",
  building: "warn",
  deploying: "warn",
  queued: "warn",
  orphan: "warn",
  inusebyunknown: "warn",
  failed: "danger",
  error: "danger",
  stopped: "muted",
  inactive: "muted",
  disabled: "muted",
};

export function StatusBadge({ status }: { status: string }) {
  const key = status.toLowerCase();
  const tone = TONE_BY_STATUS[key] ?? "muted";
  return (
    <span className={`${styles.badge} ${styles[tone]}`}>
      {LABELS[key] ?? status}
    </span>
  );
}

const LABELS: Record<string, string> = {
  git_source: "git",
  docker_image: "image",
  db_template: "db",
  inuse: "in use",
  inusebyunknown: "in use",
  orphan: "orphan",
};
