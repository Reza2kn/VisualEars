import React from 'react';

export type CardTone = 'default' | 'raised' | 'ember' | 'accent';

export interface CardProps extends React.HTMLAttributes<HTMLDivElement> {
  tone?: CardTone;
  padding?: string;
  interactive?: boolean;
}

/** Card — the workhorse surface. Soft 24px corners, warm shadow. */
export function Card({
  children,
  tone = 'default',
  padding = 'var(--pad-card)',
  interactive = false,
  style,
  ...rest
}: CardProps) {
  const tones: Record<CardTone, React.CSSProperties> = {
    default: { background: 'var(--surface-card)', color: 'var(--text-body)', border: '1px solid var(--border-subtle)' },
    raised: { background: 'var(--surface-raised)', color: 'var(--text-body)', border: '1px solid var(--border-subtle)' },
    ember: { background: 'var(--grad-ember)', color: 'var(--cream-100)', border: '1px solid transparent' },
    accent: { background: 'var(--gold-50)', color: 'var(--text-body)', border: '1px solid var(--border-accent)' },
  };
  const t = tones[tone] ?? tones.default;
  return (
    <div
      className="ve-card"
      style={{
        borderRadius: 'var(--radius-card)',
        padding,
        boxShadow: tone === 'raised' ? 'var(--shadow-md)' : 'var(--shadow-sm)',
        transition: 'transform .18s ease, box-shadow .18s ease',
        cursor: interactive ? 'pointer' : 'default',
        ...t, ...style,
      }}
      onMouseEnter={interactive ? (e) => { e.currentTarget.style.transform = 'translateY(-2px)'; e.currentTarget.style.boxShadow = 'var(--shadow-lg)'; } : undefined}
      onMouseLeave={interactive ? (e) => { e.currentTarget.style.transform = ''; e.currentTarget.style.boxShadow = 'var(--shadow-sm)'; } : undefined}
      {...rest}
    >
      {children}
    </div>
  );
}
