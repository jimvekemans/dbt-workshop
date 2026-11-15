import type { AnchorHTMLAttributes, ReactNode } from 'react'
import { cx } from '../lib/cx'
import styles from './Button.module.css'

interface Props extends AnchorHTMLAttributes<HTMLAnchorElement> {
  variant?: 'primary' | 'secondary' | 'quiet'
  size?: 'md' | 'lg'
  icon?: ReactNode
}

/** A link that looks like a button (hash navigation, so it works with the keyboard and middle-click). */
export function LinkButton({ variant = 'secondary', size = 'md', icon, className, children, ...rest }: Props) {
  return (
    <a {...rest} className={cx(styles.button, styles[variant], styles[size], className)}>
      {icon}
      <span>{children}</span>
    </a>
  )
}
