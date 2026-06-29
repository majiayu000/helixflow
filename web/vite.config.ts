import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const env = (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } })
  .process?.env;
const apiTarget = env?.HELIXFLOW_API_TARGET ?? 'http://127.0.0.1:8787';
const wsTarget = apiTarget.replace(/^http/, 'ws');

export default defineConfig({
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: 5173,
    proxy: {
      '/api': apiTarget,
      '/ws': {
        target: wsTarget,
        ws: true,
      },
    },
  },
});
