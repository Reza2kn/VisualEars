import React from 'react';

export type IconButtonVariant = 'soft' | 'solid' | 'accent' | 'ghost';
export type IconButtonSize = 'sm' | 'md' | 'lg';

export interface IconButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  children?: React.ReactNode;
  /** Accessible label (aria-label + title). */
  label: string;
  variant?: IconButtonVariant;
  size?: IconButtonSize;
}

/** Circular icon-only button. */
export function IconButton({
  children,
  label,
  variant = 'soft',
  size = 'md',
  disabled = false,
  onClick,
  style,
  ...rest
}: IconButtonProps) {
  const sizes: Record<IconButtonSize, number> = { sm: 40, md: 52, lg: 64 };
  const dim = sizes[size] ?? sizes.md;
  const variants: Record<IconButtonVariant, React.CSSProperties> = {
    soft: { background: 'var(--surface-sunken)', color: 'var(--text-brand)' },
    solid: { background: 'var(--crimson-500)', color: 'var(--text-on-brand)' },
    accent: { background: 'var(--gold-300)', color: 'var(--text-on-accent)' },
    ghost: { background: 'transparent', color: 'var(--text-body)' },
  };
  const v = variants[variant] ?? variants.soft;
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="ve-icon-button"
      style={{
        width: dim, height: dim,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        borderRadius: 'var(--radius-pill)',
        border: '1px solid transparent',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1,
        transition: 'transform .16s ease, filter .16s ease',
        ...v, ...style,
      }}
      onMouseDown={(e) => { if (!disabled) e.currentTarget.style.transform = 'scale(0.92)'; }}
      onMouseUp={(e) => { e.currentTarget.style.transform = ''; }}
      onMouseLeave={(e) => { e.currentTarget.style.transform = ''; e.currentTarget.style.filter = ''; }}
      onMouseEnter={(e) => { if (!disabled) e.currentTarget.style.filter = 'brightness(0.93)'; }}
      {...rest}
    >
      {children}
    </button>
  );
}
