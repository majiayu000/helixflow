import { describe, expect, it } from 'vitest';
import { isBenignViteWsProxyError } from './vite-ws-proxy-log';

describe('isBenignViteWsProxyError', () => {
  it('filters Vite ws proxy EPIPE and ECONNRESET, including ANSI-colored labels', () => {
    const red = (value: string) => `\u001b[31m${value}\u001b[39m`;
    expect(isBenignViteWsProxyError(`${red('ws proxy error:')}\nError: write EPIPE`, {
      code: 'EPIPE',
      stack: 'Error: write EPIPE',
    })).toBe(true);
    expect(isBenignViteWsProxyError(`${red('ws proxy socket error:')}\nError: read ECONNRESET`, {
      code: 'ECONNRESET',
    })).toBe(true);
  });

  it('keeps real backend and HTTP proxy failures visible', () => {
    expect(isBenignViteWsProxyError('ws proxy error:\nError: connect ECONNREFUSED', {
      code: 'ECONNREFUSED',
    })).toBe(false);
    expect(isBenignViteWsProxyError('http proxy error: /api/workspaces\nError: write EPIPE', {
      code: 'EPIPE',
    })).toBe(false);
    expect(isBenignViteWsProxyError('ws proxy error:\nsomething else')).toBe(false);
  });
});
