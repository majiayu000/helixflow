import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';

const env = (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } })
  .process?.env;
const apiTarget = env?.HELIXFLOW_API_TARGET ?? 'http://127.0.0.1:8787';
const wsTarget = apiTarget.replace(/^http/, 'ws');
const here = path.dirname(fileURLToPath(import.meta.url));
const UMD_ROOT = '(typeof window !== "undefined" ? window : globalThis, () => {';

function findCuterImageProcessors(): string {
  let dir = here;
  for (let index = 0; index < 8; index += 1) {
    const candidate = path.resolve(dir, 'cuter/01/image-processors.js');
    if (fs.existsSync(candidate)) return candidate;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(
    'Cuter image-processors.js not found. Helixflow 宫格/扩图/擦除引用该文件，不复制算法。',
  );
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
      const sourcePath = findCuterImageProcessors();
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
