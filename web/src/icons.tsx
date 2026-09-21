import type { CSSProperties, ReactNode } from 'react';

export type IconName =
  | 'alert'
  | 'arrowUp'
  | 'bolt'
  | 'check'
  | 'dice'
  | 'eraser'
  | 'export'
  | 'folder'
  | 'grid'
  | 'hand'
  | 'image'
  | 'layers'
  | 'lock'
  | 'map'
  | 'music'
  | 'paperclip'
  | 'palette'
  | 'play'
  | 'refresh'
  | 'redo'
  | 'spark'
  | 'stop'
  | 'sliders'
  | 'text'
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
  eraser: (
    <g>
      <path d="M4 10.8h5.7" />
      <path d="M3 7.8 7.8 3a1 1 0 0 1 1.4 0L11 4.8a1 1 0 0 1 0 1.4L6.8 10.4a1.3 1.3 0 0 1-1.8 0L3 8.4a.4.4 0 0 1 0-.6z" />
      <path d="M6.2 4.6 9.4 7.8" />
    </g>
  ),
  export: (
    <g>
      <path d="M7 9V2.5M4.5 5 7 2.5 9.5 5" />
      <path d="M3 9.5v1.5a.8.8 0 0 0 .8.8h6.4a.8.8 0 0 0 .8-.8V9.5" />
    </g>
  ),
  folder: (
    <g>
      <path d="M2.2 5h9.6v5.5a.9.9 0 0 1-.9.9H3.1a.9.9 0 0 1-.9-.9z" />
      <path d="M2.2 5V3.7a.9.9 0 0 1 .9-.9h2.4l1.1 1.1h4.3a.9.9 0 0 1 .9.9V5" />
    </g>
  ),
  hand: (
    <g>
      <path d="M4.1 7.2V3.9a.8.8 0 0 1 1.6 0V7" />
      <path d="M5.7 6V3a.8.8 0 0 1 1.6 0v3" />
      <path d="M7.3 6.1V3.7a.8.8 0 0 1 1.6 0v2.7" />
      <path d="M8.9 6.8V5.3a.8.8 0 0 1 1.6 0v3.1A3.6 3.6 0 0 1 6.9 12h-.5a3.4 3.4 0 0 1-2.6-1.2L2.2 8.9a.8.8 0 0 1 1.1-1.1l.8.7" />
    </g>
  ),
  grid: (
    <g>
      <rect x="2.5" y="2.5" width="9" height="9" rx="1" />
      <path d="M2.5 5.5h9M2.5 8.5h9M5.5 2.5v9M8.5 2.5v9" />
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
  map: (
    <g>
      <rect x="2.2" y="3.2" width="9.6" height="7.6" rx="1.4" />
      <rect x="4.1" y="4.8" width="3.4" height="2.4" rx=".5" />
    </g>
  ),
  music: (
    <g>
      <path d="M8.5 2.4v6.7" />
      <path d="M8.5 3.1 11.2 4" />
      <circle cx="5.8" cy="9.4" r="1.4" />
    </g>
  ),
  paperclip: <path d="M9.5 5.5 5.8 9.2a1.6 1.6 0 0 1-2.3-2.3l3.8-3.8a2.6 2.6 0 0 1 3.7 3.7L7.2 10.4" />,
  palette: (
    <g>
      <path d="M7 2.2a4.8 4.8 0 0 0 0 9.6h1a1 1 0 0 0 .5-1.9.8.8 0 0 1 .4-1.5h.8A2.2 2.2 0 0 0 11.8 6 4.8 4.8 0 0 0 7 2.2z" />
      <circle cx="5" cy="5" r=".55" />
      <circle cx="7.2" cy="4.2" r=".55" />
      <circle cx="9.1" cy="5.4" r=".55" />
    </g>
  ),
  play: <path d="M4.5 3v8l6-4z" />,
  refresh: <path d="M10.5 4.5A4 4 0 1 0 11 8M10.5 2.5v2h-2" />,
  redo: <path d="M9.5 6.5 11.5 8.5 9.5 10.5M11.2 8.5H5.8a2.7 2.7 0 0 1 0-5.4H8" />,
  spark: <path d="M7 1.5l1.2 3.3 3.3 1.2-3.3 1.2L7 12.5 5.8 7.2 2.5 6l3.3-1.2L7 1.5z" />,
  stop: <rect x="3.5" y="3.5" width="7" height="7" rx="1.3" />,
  sliders: (
    <g>
      <path d="M3 4h8M3 10h8" />
      <circle cx="5.4" cy="4" r="1.1" />
      <circle cx="8.6" cy="10" r="1.1" />
    </g>
  ),
  text: (
    <g>
      <path d="M3.2 3h7.6" />
      <path d="M7 3v8" />
      <path d="M5.1 11h3.8" />
    </g>
  ),
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
