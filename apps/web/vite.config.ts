import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In dev the console is served by Vite while tock-server runs elsewhere; this
// proxy makes the API same-origin (`/v1`, `/health`, `/metrics`) exactly as it
// is in production behind the reverse proxy. Override the target with
// TOCK_SERVER_PROXY (default: local server on :8787).
const apiTarget = process.env.TOCK_SERVER_PROXY ?? "http://localhost:8787";

export default defineConfig({
  plugins: [react()],
  // The wasm package is local; allow Vite to serve its files outside root.
  server: {
    fs: { allow: [".", "../../crates/tock-wasm"] },
    proxy: {
      "/v1": { target: apiTarget, changeOrigin: true },
      "/health": { target: apiTarget, changeOrigin: true },
      "/metrics": { target: apiTarget, changeOrigin: true },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: [],
  },
});
