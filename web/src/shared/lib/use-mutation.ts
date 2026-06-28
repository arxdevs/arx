import { useCallback, useState } from "react";
import { ApiError } from "@/shared/api";

interface MutationState<A> {
  run: (arg: A) => Promise<void>;
  loading: boolean;
  error: ApiError | undefined;
}

export function useMutation<A>(
  action: (arg: A) => Promise<unknown>,
  onSuccess?: () => void,
): MutationState<A> {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<ApiError>();

  const run = useCallback(
    async (arg: A) => {
      setLoading(true);
      setError(undefined);
      try {
        await action(arg);
        onSuccess?.();
      } catch (err: unknown) {
        setError(
          err instanceof ApiError ? err : new ApiError(0, "error", String(err)),
        );
      } finally {
        setLoading(false);
      }
    },
    [action, onSuccess],
  );

  return { run, loading, error };
}
