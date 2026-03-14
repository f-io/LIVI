import { render, screen } from '@testing-library/react'
import { About } from '../About'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fb?: string) => fb ?? key
  })
}))

describe('About page', () => {
  test('renders package metadata rows', () => {
    ;(globalThis as any).__BUILD_RUN__ = '123'
    ;(globalThis as any).__BUILD_SHA__ = 'abcdef0'

    render(<About />)
    expect(screen.getByText('LIVI')).toBeInTheDocument()
    expect(screen.getByText((v) => /^\d+\.\d+\.\d+/.test(v))).toBeInTheDocument()
    expect(screen.getByText('#123')).toBeInTheDocument()
    expect(screen.getByText('abcdef0')).toBeInTheDocument()
  })
})
