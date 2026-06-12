import React from 'react';

export type ButtonVariant = 'primary' | 'accent' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  children?: React.ReactNode;
  variant?: ButtonVariant;
  size?: ButtonSize;
  iconStart?: React.ReactNode;
  iconEnd?: React.ReactNode;
  fullWidth?: boolean;
}

/**
 * VisualEars Button — pill-shaped, warm, large by default.
 * Variants: primary (crimson), accent (saffron), secondary (outline), ghost, danger.
 */
export function Button({
  children,
  variant = 'primary',
  size = 'md',
  iconStart,
  iconEnd,
  fullWidth = false,
  disabled = false,
  type = 'button',
  onClick,
  style,
  ...rest
}: ButtonProps) {
  const sizes = {
    sm: { font: 'var(--text-sm)', pad: '0.5rem 1.1rem', gap: '0.4rem', minh: '40px' },
    md: { font: 'var(--text-md)', pad: '0.7rem 1.6rem', gap: '0.5rem', minh: '52px' },
    lg: { font: 'var(--text-lg)', pad: '0.95rem 2.2rem', gap: '0.6rem', minh: '64px' },
  } as const;
  const variants: Record<ButtonVariant, React.CSSProperties> = {
    primary: { background: 'var(--crimson-500)', color: 'var(--text-on-brand)', border: '1px solid transparent' },
    accent: { background: 'var(--gold-300)', color: 'var(--text-on-accent)', border: '1px solid transparent' },
    secondary: { background: 'var(--surface-card)', color: 'var(--text-brand)', border: '1.5px solid var(--border-brand)' },
    ghost: { background: 'transparent', color: 'var(--text-brand)', border: '1px solid transparent' },
    danger: { background: 'var(--danger)', color: '#fff', border: '1px solid transparent' },
  };
  const s = sizes[size] ?? sizes.md;
  const v = variants[variant] ?? variants.primary;

  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      className="ve-button"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: s.gap,
        font: `var(--weight-semibold) ${s.font}/1.1 var(--font-text)`,
        padding: s.pad,
        minHeight: s.minh,
        width: fullWidth ? '100%' : 'auto',
        borderRadius: 'var(--radius-pill)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1,
        transition: 'transform .16s ease, background .16s ease, box-shadow .16s ease, filter .16s ease',
        boxShadow: variant === 'primary' || variant === 'accent' || variant === 'danger' ? 'var(--shadow-sm)' : 'none',
        ...v,
        ...style,
      }}
      onMouseDown={(e) => { if (!disabled) e.currentTarget.style.transform = 'scale(0.97)'; }}
      onMouseUp={(e) => { e.currentTarget.style.transform = ''; }}
      onMouseLeave={(e) => { e.currentTarget.style.transform = ''; e.currentTarget.style.filter = ''; }}
      onMouseEnter={(e) => { if (!disabled) e.currentTarget.style.filter = 'brightness(0.93)'; }}
      {...rest}
    >
      {iconStart}
      {children}
      {iconEnd}
    </button>
  );
}
