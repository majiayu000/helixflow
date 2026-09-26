import { describe, expect, it } from 'vitest';
import { frameTimeForKind } from './video-frame';

describe('frameTimeForKind', () => {
  it('picks first, last, and current times without overshooting duration', () => {
    expect(frameTimeForKind('first', 10, 3)).toBe(0.04);
    expect(frameTimeForKind('last', 10, 3)).toBe(9.96);
    expect(frameTimeForKind('current', 10, 3)).toBe(3);
    expect(frameTimeForKind('current', 10, 99)).toBe(10);
    expect(frameTimeForKind('current', 0, 1.2)).toBe(1.2);
    expect(frameTimeForKind('first', 0, 1.2)).toBe(0);
  });
});
