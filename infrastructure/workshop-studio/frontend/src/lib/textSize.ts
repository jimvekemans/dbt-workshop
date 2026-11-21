import { useCallback, useEffect, useState } from 'react'

/** The text sizes the presenter can choose from, smallest first. `scale` multiplies the page's base size, which
 *  itself grows with the width of the window (see `html` in base.css). */
export const TEXT_SIZES = [
  { id: 'xsmall', label: 'Extra klein', scale: 0.45 },
  { id: 'small', label: 'Klein', scale: 0.55 },
  { id: 'medium', label: 'Normaal', scale: 0.65 },
  { id: 'large', label: 'Groot', scale: 0.75 },
  { id: 'xlarge', label: 'Extra groot', scale: 0.85 },
] as const
export type TextSizeId = (typeof TEXT_SIZES)[number]['id']
export const DEFAULT_TEXT_SIZE: TextSizeId = 'medium'
/** A new key on purpose: the sizes of the old table (0.65 to 1.15) must not carry over into this one (0.45 to 0.85). */
const KEY = 'studio-text-size-v2'

function stored(): TextSizeId {
  try {
    const v = localStorage.getItem(KEY)
    return TEXT_SIZES.some((s) => s.id === v) ? (v as TextSizeId) : DEFAULT_TEXT_SIZE
  } catch {
    return DEFAULT_TEXT_SIZE
  }
}

export function useTextSize() {
  const [id, setId] = useState<TextSizeId>(stored)
  const index = Math.max(0, TEXT_SIZES.findIndex((s) => s.id === id))
  const size = TEXT_SIZES[index] ?? TEXT_SIZES[2]
  useEffect(() => {
    document.documentElement.style.setProperty('--text-scale', String(size.scale))
  }, [size])
  const step = useCallback(
    (by: number) => {
      const next = TEXT_SIZES[Math.min(TEXT_SIZES.length - 1, Math.max(0, index + by))] ?? TEXT_SIZES[2]
      setId(next.id)
      try {
        localStorage.setItem(KEY, next.id)
      } catch {
        /* private window: the choice just is not remembered */
      }
    },
    [index],
  )
  return {
    size,
    canSmaller: index > 0,
    canLarger: index < TEXT_SIZES.length - 1,
    smaller: () => step(-1),
    larger: () => step(1),
  }
}
