import 'vitest'

declare module 'vitest' {
  interface Assertion {
    toBeInTheDocument(): void
    toHaveAttribute(name: string, value?: string): void
    toHaveTextContent(text: string): void
    toHaveFocus(): void
  }
  interface AsymmetricMatchersContaining {
    toBeInTheDocument(): void
  }
}
