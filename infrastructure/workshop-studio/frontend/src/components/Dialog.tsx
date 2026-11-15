import { useEffect, useId, useRef } from 'react'
import type { ReactNode } from 'react'
import { createPortal } from 'react-dom'
import styles from './Dialog.module.css'

interface Props {
  open: boolean
  title: string
  onClose: () => void
  children?: ReactNode
  actions?: ReactNode
  wide?: boolean
  /** `full`: a big dialog for a document (a mail). */
  size?: 'normal' | 'full'
  /** Which element gets focus first: the dialog itself (default), or the first control. */
  initialFocus?: 'dialog' | 'first'
}

const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),select,textarea,[tabindex]:not([tabindex="-1"])'

export function Dialog({ open, title, onClose, children, actions, wide, size = 'normal', initialFocus = 'first' }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const titleId = useId()
  const returnTo = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!open) return
    returnTo.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const el = ref.current
    const first = initialFocus === 'first' ? el?.querySelector<HTMLElement>(FOCUSABLE) : null
    ;(first ?? el)?.focus()
    // the page behind a dialog does not scroll
    const html = document.documentElement
    const was = html.style.overflow
    html.style.overflow = 'hidden'
    return () => {
      html.style.overflow = was
      returnTo.current?.focus?.()
    }
  }, [open, initialFocus])

  if (!open) return null

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      onClose()
    } else if (e.key === 'Tab' && ref.current) {
      const items = Array.from(ref.current.querySelectorAll<HTMLElement>(FOCUSABLE))
      if (items.length === 0) return
      const first = items[0]!
      const last = items[items.length - 1]!
      if (e.shiftKey && (document.activeElement === first || document.activeElement === ref.current)) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }
  }

  return createPortal(
    <div className={`${styles.overlay} ${size === 'full' ? styles.overlayFull : ''}`} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={ref}
        className={`${styles.dialog} ${wide ? styles.wide : ''} ${size === 'full' ? styles.full : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        {children && <div className={styles.body}>{children}</div>}
        {actions && <div className={styles.actions}>{actions}</div>}
      </div>
    </div>,
    document.body,
  )
}
