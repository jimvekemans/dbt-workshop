import type { ReactNode, Ref } from 'react'
import { cx } from '../lib/cx'
import { StatusMark } from './Status'
import { InfoIcon } from './icons'
import type { StatusState } from './Status'
import styles from './Callout.module.css'

interface Props {
  tone?: 'info' | 'good' | 'bad' | 'warn'
  title?: string
  children?: ReactNode
  actions?: ReactNode
  role?: 'status' | 'alert' | 'note'
  className?: string
  /** For moving focus to the box when something finished. */
  boxRef?: Ref<HTMLDivElement>
}
const MARK: Record<NonNullable<Props['tone']>, StatusState> = { info: 'idle', good: 'ok', bad: 'bad', warn: 'warn' }

export function Callout({ tone = 'info', title, children, actions, role = 'note', className, boxRef }: Props) {
  return (
    <div ref={boxRef} tabIndex={boxRef ? -1 : undefined} className={cx(styles.callout, styles[tone], className)} role={role}>
      <span className={styles.mark}>
        {tone === 'info' ? <InfoIcon size="1.4em" /> : <StatusMark state={MARK[tone]} size="1.4em" />}
      </span>
      <div className={styles.body}>
        {title && <p className={styles.title}>{title}</p>}
        {children && <div className={styles.text}>{children}</div>}
      </div>
      {actions && <div className={styles.actions}>{actions}</div>}
    </div>
  )
}
