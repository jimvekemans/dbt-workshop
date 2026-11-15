import { useStudio } from '../lib/studio'
import { Callout } from './Callout'

/** What the last job wants the presenter to know (the backend clears it when the next job starts). */
export function NoticeBar() {
  const { server } = useStudio()
  const notice = server?.notice
  // in a replay nothing is changed, so "nothing was changed" notices say nothing new: the amber banner on the demo says it
  if (!notice?.text || (server?.mode === 'replay' && notice.level === 'info' && /replay/i.test(notice.text))) return null
  return (
    <div style={{ marginBottom: 'var(--space-5)' }}>
      <Callout tone={notice.level === 'problem' ? 'bad' : 'info'} role={notice.level === 'problem' ? 'alert' : 'status'} title={notice.level === 'problem' ? 'Something needs your attention' : 'Good to know'}>
        {notice.text}
      </Callout>
    </div>
  )
}
