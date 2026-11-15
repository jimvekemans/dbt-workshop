import { plainText } from '../lib/glossary'
import type { Scenario } from '../api/types'
import { CodeBlock } from '../components/CodeBlock'
import { RichText } from '../components/Term'
import { partTitle } from '../lib/router'
import styles from './parts.module.css'

export function CodePart({ scenario }: { scenario: Scenario }) {
  const many = scenario.code.length > 1
  const links = scenario.code.some((b) => b.gitlab_url)
  return (
    <>
      <div className={styles.head}>
        <p className="kicker">{partTitle('code')}</p>
        <h2 className={styles.h2} id="part-heading" tabIndex={-1}>
          The lines that do it
        </h2>
        <p className={styles.lead}>
          You do not need to read the code. Read the numbered notes; each one sits under the line it explains.
          {links && ' “Open in GitLab” needs the workshop login.'}
        </p>
      </div>
      <div className={styles.blocks}>
        {scenario.code.map((b, i) => (
          <section key={i} className={styles.block} aria-label={plainText(b.title)}>
            <h3 className={styles.blockTitle}>
              {many && (
                <>
                  <span className="kicker">
                    Block {i + 1} of {scenario.code.length}
                  </span>
                  <br />
                </>
              )}
              <RichText text={b.title} />
            </h3>
            <CodeBlock block={b} />
          </section>
        ))}
      </div>
    </>
  )
}
