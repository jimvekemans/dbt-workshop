import { plainText } from '../lib/glossary'
import type { Scenario } from '../api/types'
import { CodeBlock } from '../components/CodeBlock'
import { JumpNav } from '../components/JumpNav'
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
          De regels die het doen
        </h2>
        <p className={styles.lead}>
          Je moet de code niet lezen. Lees de genummerde opmerkingen; elke opmerking staat onder de regel die ze uitlegt.
          {links && ' “Openen in GitLab” vraagt de workshoplogin.'}
        </p>
      </div>
      {scenario.code.length > 3 && (
        <JumpNav label="De codeblokken van dit scenario" items={scenario.code.map((b, i) => ({ target: `block-title-${i}`, title: plainText(b.title), mark: String(i + 1) }))} />
      )}
      <div className={styles.blocks}>
        {scenario.code.map((b, i) => (
          <section key={i} className={styles.block} aria-label={plainText(b.title)}>
            <h3 className={styles.blockTitle} id={`block-title-${i}`} tabIndex={-1}>
              {many && (
                <>
                  <span className="kicker">
                    Blok {i + 1} van {scenario.code.length}
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
