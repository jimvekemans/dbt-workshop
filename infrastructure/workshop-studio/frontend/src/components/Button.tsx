import type { ButtonHTMLAttributes, MouseEvent, ReactNode } from 'react'
import { cx } from '../lib/cx'
import styles from './Button.module.css'

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'type'> {
  variant?: 'primary' | 'secondary' | 'quiet' | 'danger'
  size?: 'md' | 'lg'
  loading?: boolean
  /** Why the button cannot be used right now. Shown as a tooltip; callers also show it as visible text. */
  disabledReason?: string
  icon?: ReactNode
}

/** A disabled button stays focusable (aria-disabled) so the reason is reachable by keyboard and screen reader. */
export function Button({
  variant = 'secondary',
  size = 'md',
  loading,
  disabled,
  disabledReason,
  icon,
  className,
  onClick,
  children,
  title,
  ...rest
}: ButtonProps) {
  const blocked = !!disabled || !!loading
  const handle = (e: MouseEvent<HTMLButtonElement>) => {
    if (blocked) {
      e.preventDefault()
      return
    }
    onClick?.(e)
  }
  return (
    <button
      type="button"
      {...rest}
      aria-disabled={blocked || undefined}
      aria-busy={loading || undefined}
      title={disabled && disabledReason ? disabledReason : title}
      className={cx(styles.button, styles[variant], styles[size], blocked && styles.blocked, className)}
      onClick={handle}
    >
      {loading ? <span className={styles.spinner} aria-hidden="true" /> : icon}
      <span>{children}</span>
    </button>
  )
}
