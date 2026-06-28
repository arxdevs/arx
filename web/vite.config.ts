import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

const API_TARGET = "http://127.0.0.1:7878";
const proxy = ["/v1", "/health", "/webhooks"].reduce<Record<string, object>>(
  (acc, route) => {
    acc[route] = { target: API_TARGET, changeOrigin: true, ws: true };
    return acc;
  },
  {},
);

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "src") },
  },
  build: {
    outDir: process.env.ARX_WEB_OUT_DIR ?? "../crates/arx-server/web-dist",
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy,
  },
});
