import type { Service } from "@/entities/service";
import { Card } from "@/shared/ui";
import styles from "./overview-tab.module.css";

function sourceSummary(s: Service): string {
  const src = s.source ?? {};
  if (s.kind === "git_source") {
    return `${src.github_repo ?? "?"}${src.branch ? `@${src.branch}` : ""}`;
  }
  if (s.kind === "docker_image") return src.image ?? "?";
  if (s.kind === "db_template") return src.template ?? "?";
  return "—";
}

export function OverviewTab({ service }: { service: Service }) {
  const rows: Array<[string, string]> = [
    ["Kind", service.kind],
    ["Source", sourceSummary(service)],
    ["Build command", service.build_command || "auto"],
    ["Start command", service.start_command || "auto"],
    ["Pre-deploy", service.pre_deploy_command || "—"],
    ["Restart policy", service.restart_policy],
  ];

  return (
    <Card>
      <dl className={styles.grid}>
        {rows.map(([label, value]) => (
          <div key={label} className={styles.row}>
            <dt className={styles.label}>{label}</dt>
            <dd className={styles.value}>{value}</dd>
          </div>
        ))}
      </dl>
    </Card>
  );
}
