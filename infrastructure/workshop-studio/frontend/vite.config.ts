import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// `npm run dev` proxies the API + websocket to the studio backend (uvicorn on 127.0.0.1:8085).
// The backend runs fine without the Docker stack: Sim mode needs nothing but the scenario YAML.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8085",
      "/ws": { target: "ws://127.0.0.1:8085", ws: true },
    },
  },
  build: { outDir: "dist", sourcemap: false },
  test: { environment: "jsdom", include: ["src/**/*.test.ts", "src/**/*.test.tsx"] },
});
