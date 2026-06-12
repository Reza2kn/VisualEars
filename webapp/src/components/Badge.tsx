import React from 'react';

export type BadgeTone = 'neutral' | 'brand' | 'accent' | 'positive' | 'warning' | 'danger';

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  tone?: BadgeTone;
  dot?: boolean;
}

/** Badge / pill label. Use `dot` for a status indicator. */
export function Badge({ children, tone = 'neutral', dot = false, style, ...rest }: BadgeProps) {
  const tones: Record<BadgeTone, { bg: string; fg: string }> = {
    neutral: { bg: 'var(--surface-sunken)', fg: 'var(--text-muted)' },
    brand: { bg: 'var(--crimson-50)', fg: 'var(--crimson-600)' },
    accent: { bg: 'var(--gold-100)', fg: 'var(--gold-700)' },
    positive: { bg: '#E8EEDC', fg: '#3F5226' },
    warning: { bg: 'var(--gold-100)', fg: 'var(--gold-700)' },
    danger: { bg: '#F6DCDA', fg: '#7A1F1A' },
  };
  const t = tones[tone] ?? tones.neutral;
  return (
    <span
      className="ve-badge"
      style={{
        display: 'inline-flex', alignItems: 'center', gap: '0.4rem',
        font: `var(--weight-semibold) var(--text-xs)/1 var(--font-text)`,
        padding: '0.35rem 0.75rem',
        borderRadius: 'var(--radius-pill)',
        background: t.bg, color: t.fg,
        ...style,
      }}
      {...rest}
    >
      {dot && <span style={{ width: 8, height: 8, borderRadius: '50%', background: 'currentColor' }} />}
      {children}
    </span>
  );
}
