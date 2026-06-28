import { serviceApi } from "@/entities/service";
import { Button } from "@/shared/ui";
import { useMutation } from "@/shared/lib";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
  onDone: () => void;
}

export function ServiceActions({ ws, proj, svc, env, onDone }: Props) {
  const deploy = useMutation(
    () => serviceApi.deploy(ws, proj, svc, env),
    onDone,
  );
  const restart = useMutation(
    () => serviceApi.restart(ws, proj, svc, env),
    onDone,
  );

  return (
    <>
      <Button
        variant="primary"
        loading={deploy.loading}
        onClick={() => deploy.run(undefined)}
      >
        Deploy
      </Button>
      <Button loading={restart.loading} onClick={() => restart.run(undefined)}>
        Restart
      </Button>
    </>
  );
}
