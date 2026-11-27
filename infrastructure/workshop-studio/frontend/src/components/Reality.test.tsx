import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Reality } from './Reality'

describe('what really happened ("Bij ons gebeurd")', () => {
  it('shows the line under its heading', () => {
    render(<Reality text="In één release gingen 15 van de 20 wijzigingen er weer uit." />)
    const box = screen.getByRole('complementary', { name: 'Bij ons gebeurd' })
    expect(box).toHaveTextContent('15 van de 20 wijzigingen')
  })
  it('draws nothing without a line', () => {
    const { container } = render(
      <>
        <Reality text={null} />
        <Reality text="   " />
        <Reality />
      </>,
    )
    expect(container.innerHTML).toBe('')
  })
})
