import { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import styles from './Toast.module.css'

type Tone = 'info' | 'bad' | 'good'
interface ToastItem {
  id: number
  tone: Tone
  text: string
}
interface ToastApi {
  show: (text: string, tone?: Tone) => void
}

const Ctx = createContext<ToastApi>({ show: () => {} })
export const useToast = (): ToastApi => useContext(Ctx)

const LABEL: Record<Tone, string> = { info: 'Note', bad: 'Problem', good: 'Done' }

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([])
  const next = useRef(1)
  const dismiss = useCallback((id: number) => setItems((xs) => xs.filter((x) => x.id !== id)), [])
  const show = useCallback(
    (text: string, tone: Tone = 'info') => {
      const id = next.current++
      setItems((xs) => [...xs.filter((x) => x.text !== text).slice(-3), { id, tone, text }])
      setTimeout(() => dismiss(id), tone === 'bad' ? 9000 : 5000)
    },
    [dismiss],
  )
  const api = useMemo(() => ({ show }), [show])
  return (
    <Ctx.Provider value={api}>
      {children}
      <div className={styles.region} role="status" aria-live="polite">
        {items.map((t) => (
          <div key={t.id} className={`${styles.toast} ${styles[t.tone]}`}>
            <span className={styles.label}>{LABEL[t.tone]}</span>
            <span className={styles.text}>{t.text}</span>
            <button type="button" className={styles.close} onClick={() => dismiss(t.id)} aria-label="Dismiss this message">
              ×
            </button>
          </div>
        ))}
      </div>
    </Ctx.Provider>
  )
}
