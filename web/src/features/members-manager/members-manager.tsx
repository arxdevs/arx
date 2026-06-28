import { useState } from "react";
import { memberApi, type Member } from "@/entities/member";
import {
  Card,
  Field,
  Select,
  Button,
  Spinner,
  ErrorMessage,
  DataTable,
  StatusBadge,
} from "@/shared/ui";
import { useQuery, useMutation } from "@/shared/lib";
import styles from "./members-manager.module.css";

export function MembersManager({ ws }: { ws: string }) {
  const members = useQuery<Member[]>(() => memberApi.list(ws), [ws]);
  const [login, setLogin] = useState("");
  const [role, setRole] = useState("member");

  const invite = useMutation(
    () => memberApi.invite(ws, { github_login: login, role }),
    () => {
      setLogin("");
      members.reload();
    },
  );
  const remove = useMutation(
    (userId: string) => memberApi.remove(ws, userId),
    members.reload,
  );

  return (
    <Card>
      <h3 className="arx-section-title">Members</h3>
      {members.loading && <Spinner label="loading members" />}
      {members.error && <ErrorMessage message={members.error.message} />}
      {members.data && (
        <DataTable
          rowKey={(m) => m.user_id}
          rows={members.data}
          columns={[
            { header: "Name", cell: (m) => m.display_name },
            {
              header: "GitHub",
              cell: (m) => (m.github_login ? <code>@{m.github_login}</code> : "—"),
            },
            { header: "Role", cell: (m) => <StatusBadge status={m.role} /> },
            {
              header: "",
              align: "right",
              cell: (m) => (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => remove.run(m.user_id)}
                >
                  Remove
                </Button>
              ),
            },
          ]}
        />
      )}

      <div className={styles.invite}>
        <Field
          label="GitHub login"
          value={login}
          placeholder="octocat"
          onChange={(e) => setLogin(e.target.value)}
        />
        <Select
          label="Role"
          options={[
            { value: "member", label: "member" },
            { value: "admin", label: "admin" },
          ]}
          value={role}
          onChange={(e) => setRole(e.target.value)}
        />
        <Button
          variant="primary"
          loading={invite.loading}
          disabled={!login}
          onClick={() => invite.run(undefined)}
        >
          Invite
        </Button>
      </div>
      {invite.error && <ErrorMessage message={invite.error.message} />}
    </Card>
  );
}
