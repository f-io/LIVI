import { render } from '@testing-library/react'
import { Home } from '../Home'

describe('Home', () => {
  test('renders an empty div', () => {
    const { container } = render(<Home />)

    expect(container.firstChild).toBeInTheDocument()
    expect(container.firstChild?.nodeName).toBe('DIV')
    expect(container.firstChild).toBeEmptyDOMElement()
  })
})
