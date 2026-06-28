import { domainApi, type ServiceDomain } from "@/entities/domain";
import { DomainsManager } from "@/features/domains-manager";
import { Spinner, ErrorMessage } from "@/shared/ui";
import { useQuery } from "@/shared/lib";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
}

export function DomainsTab({ ws, proj, svc, env }: Props) {
  const { data, error, loading, reload } = useQuery<ServiceDomain[]>(
    () => domainApi.list(ws, proj, svc, env),
    [ws, proj, svc, env],
  );

  if (loading) return <Spinner label="loading domains" />;
  if (error) return <ErrorMessage message={error.message} />;

  return (
    <DomainsManager
      ws={ws}
      proj={proj}
      svc={svc}
      env={env}
      domains={data ?? []}
      onChange={reload}
    />
  );
}
