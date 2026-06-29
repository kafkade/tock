import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // The wasm package is local; allow Vite to serve its files outside root.
  server: { fs: { allow: [".", "../../crates/tock-wasm"] } },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: [],
  },
});
