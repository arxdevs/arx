import { useEffect, useState } from "react";
import { serverApi, type ServerSettings } from "@/entities/server";
import { Breadcrumbs } from "@/widgets/breadcrumbs";
import {
  PageHeader,
  Card,
  Field,
  Button,
  Spinner,
  ErrorMessage,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import styles from "./settings-page.module.css";

export function SettingsPage() {
  const { data, loading, error, reload } = useQuery<ServerSettings>(
    () => serverApi.settings(),
    [],
  );

  const [form, setForm] = useState<ServerSettings>({
    admin_domain: "",
    acme_email: "",
    public_ip: "",
  });

  useEffect(() => {
    if (data) {
      setForm({
        admin_domain: data.admin_domain ?? "",
        acme_email: data.acme_email ?? "",
        public_ip: data.public_ip ?? "",
      });
    }
  }, [data]);

  const save = useMutation(() => serverApi.updateSettings(form), reload);
  const certRetry = useMutation(() => serverApi.certRetry());
  const githubSync = useMutation(() => serverApi.githubSync());

  return (
    <>
      <Breadcrumbs items={[{ label: "Workspaces", to: "/" }, { label: "Settings" }]} />
      <PageHeader title="Server settings" />

      {loading && <Spinner label="loading settings" />}
      {error && <ErrorMessage message={error.message} />}

      {data && (
        <div className="arx-stack">
          <Card>
            <div className={styles.form}>
              <Field
                label="Admin domain"
                value={form.admin_domain ?? ""}
                placeholder="arx.example.com"
                onChange={(e) =>
                  setForm({ ...form, admin_domain: e.target.value })
                }
              />
              <Field
                label="ACME email"
                value={form.acme_email ?? ""}
                placeholder="you@example.com"
                onChange={(e) =>
                  setForm({ ...form, acme_email: e.target.value })
                }
              />
              <Field
                label="Public IP"
                value={form.public_ip ?? ""}
                placeholder="203.0.113.10"
                onChange={(e) =>
                  setForm({ ...form, public_ip: e.target.value })
                }
              />
              {save.error && <ErrorMessage message={save.error.message} />}
              <div className={styles.actions}>
                <Button
                  variant="primary"
                  loading={save.loading}
                  onClick={() => save.run(undefined)}
                >
                  Save
                </Button>
              </div>
            </div>
          </Card>

          <Card>
            <h2 className="arx-section-title">Maintenance</h2>
            <div className={styles.maintenance}>
              <Button
                loading={certRetry.loading}
                onClick={() => certRetry.run(undefined)}
              >
                Retry failed certificates
              </Button>
              <Button
                loading={githubSync.loading}
                onClick={() => githubSync.run(undefined)}
              >
                Sync GitHub installations
              </Button>
            </div>
          </Card>
        </div>
      )}
    </>
  );
}
