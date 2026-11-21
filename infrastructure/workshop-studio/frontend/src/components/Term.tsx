import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties, ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { lookupTerm, parseGlossary } from '../lib/glossary'
import styles from './Term.module.css'

interface TermCtx {
  glossary: Record<string, string>
  openId: string | null
  setOpenId: (id: string | null) => void
}
const Ctx = createContext<TermCtx>({ glossary: {}, openId: null, setOpenId: () => {} })

/** Holds the glossary and which popover is open: only one can be open at a time. */
export function TermProvider({ glossary, children }: { glossary: Record<string, string>; children: ReactNode }) {
  const [openId, setOpenId] = useState<string | null>(null)
  const value = useMemo(() => ({ glossary, openId, setOpenId }), [glossary, openId])
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

/** Inside this, a [[term]] is only its word, never a button: for a drawing that sits under a link or is only a preview. */
const PlainCtx = createContext(false)
export function PlainTerms({ children }: { children: ReactNode }) {
  return <PlainCtx.Provider value={true}>{children}</PlainCtx.Provider>
}

export function Term({ term, label }: { term: string; label?: string }) {
  const { glossary, openId, setOpenId } = useContext(Ctx)
  const plain = useContext(PlainCtx)
  const id = useId()
  const btn = useRef<HTMLButtonElement>(null)
  const pop = useRef<HTMLSpanElement>(null)
  const explanation = lookupTerm(glossary, term)
  const open = openId === id

  const close = useCallback(() => setOpenId(null), [setOpenId])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        close()
        btn.current?.focus()
      }
    }
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node
      if (!btn.current?.contains(t) && !pop.current?.contains(t)) close()
    }
    document.addEventListener('keydown', onKey, true)
    document.addEventListener('mousedown', onDown)
    return () => {
      document.removeEventListener('keydown', onKey, true)
      document.removeEventListener('mousedown', onDown)
    }
  }, [open, close])

  // The popover lives in document.body (nothing can clip or cover it) and is placed from the term's own box: below it when
  // there is room, otherwise above it, and always inside the viewport.
  const [pos, setPos] = useState<CSSProperties | null>(null)
  useLayoutEffect(() => {
    if (!open) {
      setPos(null)
      return
    }
    const place = () => {
      const b = btn.current
      const p = pop.current
      if (!b || !p) return
      const r = b.getBoundingClientRect()
      const w = p.offsetWidth
      const h = p.offsetHeight
      const margin = 12
      const gap = 8
      const left = Math.max(margin, Math.min(r.left, window.innerWidth - margin - w))
      const below = r.bottom + gap
      // above the term when there is room (what comes next stays readable), otherwise below
      const above = r.top - gap - h
      const top = above >= margin + 56 ? above : below
      setPos({ left, top: Math.max(margin, Math.min(top, window.innerHeight - margin - h)) })
    }
    place()
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
    return () => {
      window.removeEventListener('resize', place)
      window.removeEventListener('scroll', place, true)
    }
  }, [open])

  const shown = label ?? term
  if (!explanation || plain) return <>{shown}</>

  return (
    <span className={styles.wrap}>
      <button
        ref={btn}
        type="button"
        className={styles.term}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => setOpenId(open ? null : id)}
      >
        {shown}
      </button>
      {open &&
        createPortal(
          <span ref={pop} id={id} role="dialog" aria-label={`Wat “${term}” betekent`} className={styles.pop} style={pos ?? { left: 0, top: 0, visibility: 'hidden' }}>
            <span className={styles.popTitle}>{term}</span>
            <span className={styles.popText}>{explanation}</span>
            <span className={styles.popFoot}>
              <span aria-hidden="true">Esc sluit dit</span>
              <button type="button" className={styles.popClose} onClick={() => { close(); btn.current?.focus() }}>
                Sluiten
              </button>
            </span>
          </span>,
          document.body,
        )}
    </span>
  )
}

/** Plain text with [[terms]] turned into clickable glossary terms. */
export function RichText({ text }: { text: string }) {
  const segs = useMemo(() => parseGlossary(text), [text])
  return (
    <>
      {segs.map((s, i) => (s.kind === 'text' ? <span key={i}>{s.text}</span> : <Term key={i} term={s.term} label={s.label} />))}
    </>
  )
}

/** Paragraphs separated by blank lines, each with glossary terms. */
export function Prose({ text, className }: { text: string; className?: string }) {
  const paras = text.trim().split(/\n\s*\n/)
  return (
    <div className={className}>
      {paras.map((p, i) => (
        <p key={i}>
          <RichText text={p.replace(/\s*\n\s*/g, ' ')} />
        </p>
      ))}
    </div>
  )
}
