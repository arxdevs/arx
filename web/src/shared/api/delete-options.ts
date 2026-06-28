export interface DeleteOptions {
  force?: boolean;
  with_data?: boolean;
}

export function deleteQuery(opts?: DeleteOptions): string {
  if (!opts) return "";
  const params = new URLSearchParams();
  if (opts.force) params.set("force", "true");
  if (opts.with_data) params.set("with_data", "true");
  const qs = params.toString();
  return qs ? `?${qs}` : "";
}
