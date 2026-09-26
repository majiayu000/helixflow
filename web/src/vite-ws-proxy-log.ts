type ErrnoLike = {
  code?: string;
  message?: string;
  stack?: string;
};

export function isBenignViteWsProxyError(msg: string, error?: ErrnoLike | Error | null): boolean {
  const errno = error && typeof error === 'object' && 'code' in error
    ? String((error as ErrnoLike).code ?? '')
    : '';
  const haystack = [
    msg,
    errno,
    error && 'message' in error ? String(error.message) : '',
    error && 'stack' in error && error.stack ? String(error.stack) : '',
  ].join('\n');
  if (!/ws proxy (socket )?error/i.test(haystack)) return false;
  return errno === 'EPIPE' || errno === 'ECONNRESET' || /\b(?:EPIPE|ECONNRESET)\b/.test(haystack);
}
