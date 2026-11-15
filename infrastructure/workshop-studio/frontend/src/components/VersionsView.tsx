import type { EnvName, VersionsData } from '../api/types'
import { previousTag } from '../api/adapt'
import type { VersionTags } from '../api/adapt'
import { cx } from '../lib/cx'
import { ENV_NAME, ENV_ORDER } from '../lib/env'
import { ArrowRightIcon } from './icons'
import styles from './VersionsView.module.css'

const PLAIN: Record<EnvName, string> = {
  dev: 'where changes are tried first',
  val: 'where they are checked',
  prd: 'what the business uses',
}

/**
 * Three environments side by side. A version that changed is unmistakable: accent frame, "Changed" label, old number.
 * `previous` is what the environment ran when the step started; the backend does not send it, the app remembers it.
 */
export function VersionsView({ data, previous }: { data: VersionsData; previous?: VersionTags }) {
  const byEnv = new Map(data.environments.map((e) => [e.env, e]))
  return (
    <div className={styles.wrap}>
    <ul className={styles.row}>
      {ENV_ORDER.map((env) => {
        const e = byEnv.get(env)
        const tag = e?.tag ?? null
        const before = previousTag(previous, env)
        const changed = before !== undefined && before !== tag
        return (
          <li key={env} className={cx(styles.col, changed && styles.changed)}>
            <p className={styles.env}>{ENV_NAME[env]}</p>
            <p className={styles.plain}>{PLAIN[env]}</p>
            <p className={styles.tag} key={tag ?? 'none'}>
              {tag ?? 'not running'}
            </p>
            <p className={styles.delta}>
              {changed ? (
                <>
                  <b className={styles.flag}>Changed</b>
                  <span>
                    was <s>{before ?? 'nothing'}</s> <ArrowRightIcon size="0.9em" /> now <b>{tag ?? 'nothing'}</b>
                  </span>
                </>
              ) : (
                <span className={styles.same}>{previous ? 'Unchanged' : ' '}</span>
              )}
            </p>
          </li>
        )
      })}
    </ul>
    </div>
  )
}
