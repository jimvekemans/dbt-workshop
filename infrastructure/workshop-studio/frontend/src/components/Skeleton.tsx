import { cx } from '../lib/cx'
import styles from './Skeleton.module.css'

export function Skeleton({ width, height = '1em', className }: { width?: string; height?: string; className?: string }) {
  return <span aria-hidden="true" className={cx(styles.skeleton, className)} style={{ width, height }} />
}

export function SkeletonLines({ lines = 3, label = 'Loading' }: { lines?: number; label?: string }) {
  return (
    <div className={styles.lines} role="status" aria-label={label}>
      {Array.from({ length: lines }, (_, i) => (
        <Skeleton key={i} width={i === lines - 1 ? '60%' : '100%'} height="1.1em" />
      ))}
      <span className="sr-only">{label}…</span>
    </div>
  )
}
