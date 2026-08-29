import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

const env = (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } })
  .process?.env;
const apiTarget = env?.HELIXFLOW_API_TARGET ?? 'http://127.0.0.1:8787';
const wsTarget = apiTarget.replace(/^http/, 'ws');

export default defineConfig(({ mode }) => ({
  plugins: [react()],
  test: {
    include: ['src/**/*.test.{ts,tsx}'],
  },
  build: {
    rolldownOptions: {
      input: mode === 'e2e'
        ? { app: 'index.html', canvas: 'e2e/canvas.html' }
        : undefined,
      output: {
        codeSplitting: {
          groups: [{ name: 'vendor', test: /node_modules/ }],
        },
      },
    },
  },
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
}));
