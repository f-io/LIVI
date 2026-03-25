import { render, screen } from '@testing-library/react'
import { About } from '../About'
;(globalThis as any).__BUILD_RUN__ = '123'
;(globalThis as any).__BUILD_SHA__ = 'abcdef0'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fb?: string) => fb ?? key
  })
}))

describe('About page', () => {
  test('renders package metadata rows', () => {
    render(<About />)

    expect(screen.getByText('settings.name:')).toBeInTheDocument()
    expect(screen.getByText('settings.description:')).toBeInTheDocument()
    expect(screen.getByText('settings.version:')).toBeInTheDocument()
    expect(screen.getByText('Commit:')).toBeInTheDocument()
    expect(screen.getByText('settings.url:')).toBeInTheDocument()
    expect(screen.getByText('settings.author:')).toBeInTheDocument()
    expect(screen.getByText('settings.contributors:')).toBeInTheDocument()

    expect(screen.getByText('LIVI')).toBeInTheDocument()
    expect(screen.getByText((v) => /^\d+\.\d+\.\d+/.test(v))).toBeInTheDocument()
  })

  test('renders author and contributors rows with values', () => {
    render(<About />)

    const authorLabel = screen.getByText('settings.author:')
    const contributorsLabel = screen.getByText('settings.contributors:')

    expect(authorLabel.nextSibling).toBeInTheDocument()
    expect(contributorsLabel.nextSibling).toBeInTheDocument()
    expect(authorLabel.nextSibling?.textContent).not.toBe('')
    expect(contributorsLabel.nextSibling?.textContent).not.toBe('')
  })
})
