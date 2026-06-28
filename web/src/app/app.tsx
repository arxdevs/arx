import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { AuthProvider } from "@/features/auth";
import { AppLayout } from "@/widgets/app-layout";
import { WorkspacesPage } from "@/pages/workspaces";
import { WorkspacePage } from "@/pages/workspace";
import { ProjectPage } from "@/pages/project";
import { ServicePage } from "@/pages/service";
import { SettingsPage } from "@/pages/settings";
import { RequireAuth } from "./require-auth";

export function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <RequireAuth>
          <AppLayout>
            <Routes>
              <Route path="/" element={<WorkspacesPage />} />
              <Route path="/settings" element={<SettingsPage />} />
              <Route path="/w/:ws" element={<WorkspacePage />} />
              <Route path="/w/:ws/p/:proj" element={<ProjectPage />} />
              <Route
                path="/w/:ws/p/:proj/s/:svc"
                element={<ServicePage />}
              />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </AppLayout>
        </RequireAuth>
      </AuthProvider>
    </BrowserRouter>
  );
}
