import { variableApi, type Variable } from "@/entities/variable";
import { VariablesManager } from "@/features/variables-manager";
import { Spinner, ErrorMessage } from "@/shared/ui";
import { useQuery } from "@/shared/lib";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
}

export function VariablesTab({ ws, proj, svc, env }: Props) {
  const { data, error, loading, reload } = useQuery<Variable[]>(
    () => variableApi.list(ws, proj, svc, env),
    [ws, proj, svc, env],
  );

  if (loading) return <Spinner label="loading variables" />;
  if (error) return <ErrorMessage message={error.message} />;

  return (
    <VariablesManager
      ws={ws}
      proj={proj}
      svc={svc}
      env={env}
      variables={data ?? []}
      onChange={reload}
    />
  );
}
