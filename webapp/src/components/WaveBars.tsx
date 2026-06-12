import React from 'react';

export interface WaveBarsProps extends React.HTMLAttributes<HTMLDivElement> {
  count?: number;
  color?: string;
  height?: number;
  active?: boolean;
  /** 0–1 levels driving bar heights (e.g. real audio). Omit to auto-animate. */
  levels?: number[] | Float32Array;
}

/**
 * WaveBars — an equalizer-style sound visualizer. Renders `count` bars that
 * gently pulse. Pass `levels` (0–1 array) to drive heights from real audio,
 * or leave it to auto-animate. Calm, never strobing; `prefers-reduced-motion`
 * freezes it to low static bars (accessibility-critical).
 */
export function WaveBars({
  count = 24,
  color = 'var(--gold-300)',
  height = 64,
  active = true,
  levels,
  style,
  ...rest
}: WaveBarsProps) {
  const [tick, setTick] = React.useState(0);
  const reducedMotion = React.useMemo(
    () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  );
  React.useEffect(() => {
    if (!active || levels || reducedMotion) return;
    const id = setInterval(() => setTick((t) => t + 1), 130);
    return () => clearInterval(id);
  }, [active, levels, reducedMotion]);

  const bars = Array.from({ length: count }, (_, i) => {
    if (reducedMotion) return active ? 0.18 : 0.12;
    if (levels && levels.length) {
      return Math.max(0, Math.min(1, levels[i % levels.length]));
    }
    if (active) {
      // smooth pseudo-random envelope
      return 0.25 + 0.6 * Math.abs(Math.sin(i * 0.6 + tick * 0.5 + Math.cos(i)));
    }
    return 0.12;
  });

  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        gap: Math.max(2, Math.round(height / 28)),
        height, ...style,
      }}
      aria-hidden="true"
      {...rest}
    >
      {bars.map((h, i) => (
        <span
          key={i}
          style={{
            display: 'block',
            width: Math.max(3, Math.round(height / 14)),
            height: `${Math.round(h * 100)}%`,
            minHeight: 3,
            borderRadius: 'var(--radius-pill)',
            background: color,
            transition: 'height .16s ease',
          }}
        />
      ))}
    </div>
  );
}
