import { afterEach, expect } from 'vitest'
import { cleanup } from '@testing-library/react'

afterEach(() => cleanup())

// A few DOM matchers (the small subset the tests use) so no extra dependency is needed.
const pass = (ok: boolean, message: string) => ({ pass: ok, message: () => message })
expect.extend({
  toBeInTheDocument(el: Element | null) {
    return pass(!!el && el.ownerDocument.contains(el), 'expected the element to be in the document')
  },
  toHaveAttribute(el: Element, name: string, value?: string) {
    const has = el.hasAttribute(name)
    return pass(value === undefined ? has : has && el.getAttribute(name) === value, `expected attribute ${name}${value === undefined ? '' : `="${value}"`}`)
  },
  toHaveTextContent(el: Element, text: string) {
    return pass((el.textContent ?? '').includes(text), `expected text "${text}" in "${el.textContent}"`)
  },
  toHaveFocus(el: Element) {
    return pass(el.ownerDocument.activeElement === el, 'expected the element to have focus')
  },
})
