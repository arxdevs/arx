import { useEffect, useRef, useState } from "react";
import styles from "./log-viewer.module.css";

interface Props {
  ws: string;
  proj: string;
  svc: string;
  env?: string;
  /** Which stream to view: runtime container logs (default) or build logs. */
  kind?: "runtime" | "build";
}

interface LogLine {
  line?: string;
  ts?: string;
  error?: string;
}

export function LogViewer({ ws, proj, svc, env, kind = "runtime" }: Props) {
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">(
    "connecting",
  );
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setLines([]);
    setStatus("connecting");

    const params = new URLSearchParams({ follow: "true" });
    if (kind === "runtime") params.set("tail", "200");
    if (env) params.set("env", env);
    const path = kind === "build" ? "build-logs" : "logs";
    const url = `/v1/workspaces/${ws}/projects/${proj}/services/${svc}/${path}?${params}`;
    const source = new EventSource(url, { withCredentials: true });

    source.onopen = () => setStatus("open");
    source.onmessage = (event) => {
      try {
        const parsed: LogLine = JSON.parse(event.data);
        const text = parsed.error
          ? `[error] ${parsed.error}`
          : (parsed.line ?? "");
        setLines((prev) => [...prev.slice(-999), text]);
      } catch {
        setLines((prev) => [...prev.slice(-999), event.data]);
      }
    };
    source.onerror = () => setStatus("closed");

    return () => source.close();
  }, [ws, proj, svc, env, kind]);

  useEffect(() => {
    const box = boxRef.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [lines]);

  return (
    <div className={styles.wrap}>
      <div className={styles.bar}>
        <span className={`${styles.status} ${styles[status]}`}>{status}</span>
      </div>
      <div className={styles.box} ref={boxRef}>
        {lines.length === 0 ? (
          <span className={styles.empty}>waiting for logs…</span>
        ) : (
          lines.map((line, i) => (
            <div key={i} className={styles.line}>
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
