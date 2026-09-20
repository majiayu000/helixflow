import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createLogger, type LogErrorOptions, type Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { isBenignViteWsProxyError } from './src/vite-ws-proxy-log.ts';

const env = (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } })
  .process?.env;
const apiTarget = env?.HELIXFLOW_API_TARGET ?? 'http://127.0.0.1:8787';
const wsTarget = apiTarget.replace(/^http/, 'ws');
const here = path.dirname(fileURLToPath(import.meta.url));
const UMD_ROOT = '(typeof window !== "undefined" ? window : globalThis, () => {';
const viteLogger = createLogger();

function quietBenignWsProxyDisconnect(msg: string, options?: LogErrorOptions) {
  if (isBenignViteWsProxyError(msg, options?.error)) return;
  viteLogger.error(msg, options);
}

function cuterImageProcessors(): Plugin {
  const virtualId = 'virtual:cuter-image-processors';
  return {
    name: 'cuter-image-processors',
    resolveId(id) {
      if (id === virtualId) return id;
      return undefined;
    },
    load(id) {
      if (id !== virtualId) return undefined;
      const sourcePath = path.resolve(here, 'vendor/cuter/image-processors.js');
      const source = fs.readFileSync(sourcePath, 'utf8');
      if (!source.includes(UMD_ROOT)) {
        throw new Error('Cuter image-processors UMD shape changed; update the Helixflow adapter');
      }
      const rewritten = source
        .replace(UMD_ROOT, '(__cuterImageProcessorsRoot, () => {')
        .replace(
          'if (typeof module === "object" && module.exports) module.exports = api;',
          '',
        );
      return `const __cuterImageProcessorsRoot = {};
${rewritten}
export default __cuterImageProcessorsRoot.CuterImageProcessors;
`;
    },
  };
}

export default defineConfig(({ mode }) => ({
  plugins: [react(), cuterImageProcessors()],
  customLogger: {
    ...viteLogger,
    error: quietBenignWsProxyDisconnect,
  },
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
        // Vite 8 still logs EPIPE after configure(); customLogger filters that noise.
      },
    },
  },
}));
