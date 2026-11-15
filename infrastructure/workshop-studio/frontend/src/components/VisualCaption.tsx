import { RichText } from './Term'
import styles from './VisualCaption.module.css'

export function VisualCaption({ text }: { text?: string | null }) {
  if (!text) return null
  return (
    <figcaption className={styles.caption}>
      <RichText text={text} />
    </figcaption>
  )
}
