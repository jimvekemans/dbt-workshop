import type { HTMLAttributes, ReactNode } from 'react'
import { cx } from '../lib/cx'
import styles from './Card.module.css'

interface Props extends HTMLAttributes<HTMLElement> {
  as?: 'section' | 'article' | 'div' | 'li'
  tone?: 'plain' | 'sunk' | 'good' | 'bad' | 'warn'
  children: ReactNode
}

export function Card({ as: Tag = 'section', tone = 'plain', className, children, ...rest }: Props) {
  return (
    <Tag className={cx(styles.card, styles[tone], className)} {...rest}>
      {children}
    </Tag>
  )
}
