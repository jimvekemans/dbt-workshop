import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

// The production build is served by the studio backend from the same origin, so /api needs no CORS and no proxy.
// `npm run dev:live` (port 5198) proxies /api to the running studio backend and adds the X-Studio header the guard
// wants on every request. EventSource cannot send headers, but the backend only demands the header on requests that
// change something; reading the event stream (GET) needs only an allowed Host, so the same proxy serves /api/events.
// The Host header is kept as the browser sent it (127.0.0.1:5198), which the backend allows.
const STUDIO_API = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env?.STUDIO_API ?? 'http://127.0.0.1:8085'
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: STUDIO_API, changeOrigin: false, headers: { 'X-Studio': '1' } },
    },
  },
  build: { target: 'es2022', sourcemap: false, chunkSizeWarningLimit: 400 },
  css: { modules: { generateScopedName: '[name]_[local]_[hash:base64:4]' } },
  test: {
    environment: 'jsdom',
    setupFiles: ['src/test/setup.ts'],
    css: { modules: { classNameStrategy: 'non-scoped' } },
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
