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

    expect(screen.getByText('test-app')).toBeInTheDocument()
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

// Test the helper functions via a mock of package.json
jest.mock(
  '../../../../../../../../../package.json',
  () => ({
    name: 'test-app',
    version: '9.9.9',
    description: 'A test',
    homepage: 'https://example.com',
    author: { name: 'Jane', email: 'jane@example.com', url: 'https://jane.dev' },
    contributors: [{ name: 'Alice', email: 'alice@example.com' }, 'Bob']
  }),
  { virtual: false }
)

describe('About page with object author and contributors', () => {
  test('renders object author as formatted string (name email url)', () => {
    // lines 37-44: toAuthorString with isPersonLike object
    render(<About />)

    const authorLabel = screen.getByText('settings.author:')
    expect(authorLabel.nextSibling?.textContent).toContain('Jane')
    expect(authorLabel.nextSibling?.textContent).toContain('jane@example.com')
  })

  test('renders contributor objects and strings from array', () => {
    // lines 64-71: contributorsStr loop with objects and strings
    render(<About />)

    const contribLabel = screen.getByText('settings.contributors:')
    expect(contribLabel.nextSibling?.textContent).toContain('Alice')
    expect(contribLabel.nextSibling?.textContent).toContain('Bob')
  })
})
