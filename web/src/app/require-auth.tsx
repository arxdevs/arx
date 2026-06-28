import type { ReactNode } from "react";
import { useAuth } from "@/features/auth";
import { LoginPage } from "@/pages/login";
import { Spinner } from "@/shared/ui";

export function RequireAuth({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();

  if (loading) {
    return (
      <div style={{ display: "grid", placeItems: "center", minHeight: "100vh" }}>
        <Spinner label="loading" />
      </div>
    );
  }

  if (!user) return <LoginPage />;

  return <>{children}</>;
}
