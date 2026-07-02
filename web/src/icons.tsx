import type { CSSProperties, ReactNode } from 'react';

export type IconName =
  | 'alert'
  | 'arrowUp'
  | 'bolt'
  | 'check'
  | 'dice'
  | 'export'
  | 'image'
  | 'layers'
  | 'lock'
  | 'paperclip'
  | 'play'
  | 'refresh'
  | 'spark'
  | 'stop'
  | 'undo'
  | 'warn';

const icons: Record<IconName, ReactNode> = {
  alert: (
    <g>
      <circle cx="7" cy="7" r="4.5" />
      <path d="M7 4.8V7.4M7 9.2v.1" />
    </g>
  ),
  arrowUp: <path d="M7 11V3M3.5 6.5 7 3l3.5 3.5" />,
  bolt: <path d="M7.5 2 3.5 8h3l-1 4 4-6h-3l1-4z" />,
  check: <path d="M3 7.2 5.6 9.8 11 4.2" />,
  dice: (
    <g>
      <rect x="2.5" y="2.5" width="9" height="9" rx="2" />
      <circle cx="5" cy="5" r=".9" />
      <circle cx="9" cy="9" r=".9" />
      <circle cx="7" cy="7" r=".9" />
    </g>
  ),
  export: (
    <g>
      <path d="M7 9V2.5M4.5 5 7 2.5 9.5 5" />
      <path d="M3 9.5v1.5a.8.8 0 0 0 .8.8h6.4a.8.8 0 0 0 .8-.8V9.5" />
    </g>
  ),
  image: (
    <g>
      <rect x="2.5" y="3" width="9" height="8" rx="1.3" />
      <circle cx="5.2" cy="5.7" r="1" />
      <path d="M2.8 9.5 5.5 7l2 1.6L9 7l2.2 2.2" />
    </g>
  ),
  layers: (
    <g>
      <path d="M7 2.5 12 5 7 7.5 2 5z" />
      <path d="M2 7.5 7 10l5-2.5" />
    </g>
  ),
  lock: (
    <g>
      <rect x="3.2" y="6.2" width="7.6" height="5.3" rx="1.2" />
      <path d="M4.8 6.2V4.6a2.2 2.2 0 0 1 4.4 0v1.6" />
    </g>
  ),
  paperclip: <path d="M9.5 5.5 5.8 9.2a1.6 1.6 0 0 1-2.3-2.3l3.8-3.8a2.6 2.6 0 0 1 3.7 3.7L7.2 10.4" />,
  play: <path d="M4.5 3v8l6-4z" />,
  refresh: <path d="M10.5 4.5A4 4 0 1 0 11 8M10.5 2.5v2h-2" />,
  spark: <path d="M7 1.5l1.2 3.3 3.3 1.2-3.3 1.2L7 12.5 5.8 7.2 2.5 6l3.3-1.2L7 1.5z" />,
  stop: <rect x="3.5" y="3.5" width="7" height="7" rx="1.3" />,
  undo: <path d="M4.5 6.5 2.5 8.5 4.5 10.5M2.8 8.5h5.4a2.7 2.7 0 0 0 0-5.4H6" />,
  warn: (
    <g>
      <path d="M7 2.8 12 11H2z" />
      <path d="M7 6v2.2M7 9.6v.1" />
    </g>
  ),
};

type IconProps = {
  n: IconName;
  s?: number;
  c?: string;
  sw?: number;
  fill?: boolean;
  style?: CSSProperties;
};

export function Icon({ n, s = 14, c, sw = 1.5, fill = false, style }: IconProps) {
  return (
    <svg
      width={s}
      height={s}
      viewBox="0 0 14 14"
      fill={fill ? 'currentColor' : 'none'}
      stroke={fill ? 'none' : 'currentColor'}
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={sw}
      style={{ color: c, flexShrink: 0, ...style }}
    >
      {icons[n]}
    </svg>
  );
}

export function HistoryIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeWidth="1.5"
    >
      <circle cx="7" cy="7" r="4.8" />
      <path d="M7 4.5V7l1.8 1.2" />
    </svg>
  );
}

export function Port({ className, type = 'artifact' }: { className?: string; type?: string }) {
  return (
    <span
      className={['port', className].filter(Boolean).join(' ')}
      style={{ color: portColor(type) }}
    />
  );
}

export function portColor(type: string): string {
  const key = type.toLowerCase();
  if (key.includes('image')) return 'var(--t-image)';
  if (key.includes('video')) return 'var(--t-clip)';
  if (key.includes('latent')) return 'var(--t-latent)';
  if (key.includes('text')) return 'var(--t-cond)';
  if (key.includes('json')) return 'var(--t-mask)';
  if (key.includes('number') || key.includes('seed')) return 'var(--t-num)';
  return 'var(--accent)';
}
