import type { ButtonHTMLAttributes, ReactNode } from 'react'
import { cx } from '../lib/cx'
import styles from './IconButton.module.css'

interface Props extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'type' | 'aria-label'> {
  /** Required: icon-only buttons have no visible text, so the label is their name. */
  label: string
  children: ReactNode
  pressed?: boolean
  disabledReason?: string
}

export function IconButton({ label, children, className, pressed, disabled, disabledReason, onClick, ...rest }: Props) {
  return (
    <button
      type="button"
      {...rest}
      aria-label={label}
      aria-pressed={pressed}
      aria-disabled={disabled || undefined}
      title={disabled && disabledReason ? disabledReason : label}
      className={cx(styles.icon, disabled && styles.blocked, className)}
      onClick={(e) => {
        if (disabled) e.preventDefault()
        else onClick?.(e)
      }}
    >
      {children}
    </button>
  )
}
