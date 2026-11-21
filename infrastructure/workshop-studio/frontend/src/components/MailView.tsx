import { useEffect, useMemo, useState } from 'react'
import type { MailData } from '../api/types'
import { formatAgo, formatWhen } from '../lib/format'
import { Button } from './Button'
import { Dialog } from './Dialog'
import { EmptyNote } from './EvidenceFrame'
import { ExternalIcon } from './icons'
import styles from './MailView.module.css'

const CSP = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:">`
/** The mail's own text follows the size of the page (at least 15 px), so it reads as well as the rest of the card. */
/** A mail sets its own small sizes inline (13 px text, 11 px log lines): the whole message is enlarged so that
 *  its body text reaches the page's size instead of overriding the mail's own proportions. */
const MAIL_TEXT_PX = 13
const base = (px: number) =>
  `<style>html{background:#fff;color:#1d1b16;zoom:${(px / MAIL_TEXT_PX).toFixed(2)}}body{margin:12px;font:${MAIL_TEXT_PX}px/1.55 system-ui,sans-serif}</style>`

/** The tallest the mail's frame may be before it scrolls on its own: 70 % of the window. */
const MAX_SHARE = 0.7

function useWindowHeight(): number {
  const [h, setH] = useState(() => (typeof window === 'undefined' ? 800 : window.innerHeight))
  useEffect(() => {
    const on = () => setH(window.innerHeight)
    window.addEventListener('resize', on)
    return () => window.removeEventListener('resize', on)
  }, [])
  return h
}

/** The mail's HTML is shown in a sandboxed frame without scripts, forms or network access. */
export function MailView({ data, recorded = false }: { data: MailData; recorded?: boolean }) {
  const mail = data.found ? data : null
  const [content, setContent] = useState<number | null>(null)
  const [full, setFull] = useState(false)
  const winH = useWindowHeight()
  const root = typeof document === 'undefined' ? 16 : parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  const px = Math.max(15, Math.round(root * 0.8))
  const doc = useMemo(() => (mail ? `<!doctype html><meta charset="utf-8">${CSP}${base(px)}${mail.html}` : ''), [mail, px])
  if (!mail) return <EmptyNote>{(!data.found && data.empty) || 'Er is nog geen mail binnengekomen.'}</EmptyNote>
  const empty = !mail.html.trim()
  const max = Math.max(8 * root, Math.floor(winH * MAX_SHARE))
  // as tall as the mail, up to 70 % of the window, then the frame scrolls on its own
  const height = content ? Math.min(max, Math.max(6 * root, content)) : undefined
  const scrolls = !!content && content > max
  // no scripts run in the frame (sandbox), so reading its height is safe
  const measure = (e: { currentTarget: HTMLIFrameElement }) => {
    const h = e.currentTarget.contentDocument?.documentElement.scrollHeight
    if (h) setContent(h)
  }
  return (
    <div className={styles.wrap}>
      <div className={styles.mail}>
        <dl className={styles.head}>
          <dt>Onderwerp</dt>
          <dd className={styles.subject}>{mail.subject}</dd>
          <dt>Van</dt>
          <dd>{mail.from}</dd>
          {mail.received && (
            <>
              <dt>Ontvangen</dt>
              <dd>{recorded ? `${formatWhen(mail.received)} (in de opname)` : formatAgo(mail.received)}</dd>
            </>
          )}
        </dl>
        {empty ? (
          <p className={styles.empty}>De mail bevat geen tekst.</p>
        ) : (
          <iframe
            className={styles.frame}
            style={height ? { height } : undefined}
            title={`Mail: ${mail.subject}`}
            tabIndex={scrolls ? 0 : -1}
            sandbox="allow-same-origin"
            srcDoc={doc}
            referrerPolicy="no-referrer"
            onLoad={measure}
          />
        )}
      </div>
      {!empty && (
        <div>
          <Button icon={<ExternalIcon size="0.9em" />} onClick={() => setFull(true)}>
            Mail op volledige grootte openen
          </Button>
        </div>
      )}
      <Dialog open={full} title="De mail, op volledige grootte" onClose={() => setFull(false)} size="full" initialFocus="dialog" actions={<Button variant="primary" onClick={() => setFull(false)}>Sluiten</Button>}>
        <p className={styles.fullSubject}>{mail.subject}</p>
        <iframe className={styles.fullFrame} title={`Mail, op volledige grootte: ${mail.subject}`} sandbox="allow-same-origin" srcDoc={doc} referrerPolicy="no-referrer" tabIndex={0} />
      </Dialog>
    </div>
  )
}
