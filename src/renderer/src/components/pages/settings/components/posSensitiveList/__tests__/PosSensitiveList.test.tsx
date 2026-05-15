import type { PosListNode } from '@renderer/routes/types'
import { fireEvent, render, screen } from '@testing-library/react'
import { PosSensitiveList } from '../PosSensitiveList'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, fallback?: string) => fallback ?? key })
}))

jest.mock('../../stackItem', () => ({
  StackItem: ({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) => (
    <div data-testid="stack-item" onClick={onClick}>
      {children}
    </div>
  )
}))

jest.mock('@mui/material', () => ({
  Box: ({ children, ...rest }: { children: React.ReactNode } & Record<string, unknown>) => (
    <div {...rest}>{children}</div>
  ),
  Typography: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  IconButton: ({
    children,
    onClick,
    disabled,
    ...rest
  }: {
    children: React.ReactNode
    onClick?: (e: React.MouseEvent) => void
    disabled?: boolean
  } & Record<string, unknown>) => (
    <button type="button" disabled={disabled} onClick={onClick} {...rest}>
      {children}
    </button>
  )
}))

jest.mock('@mui/icons-material/ExpandLess', () => ({
  __esModule: true,
  default: () => <span data-testid="up-icon" />
}))
jest.mock('@mui/icons-material/ExpandMore', () => ({
  __esModule: true,
  default: () => <span data-testid="down-icon" />
}))
jest.mock('@mui/icons-material/ChevronRight', () => ({
  __esModule: true,
  default: () => <span data-testid="chev-icon" />
}))

const node: PosListNode = {
  type: 'posList',
  label: 'List',
  path: 'dashboards',
  items: [
    { id: 'a', label: 'Alpha', route: '/alpha' },
    { id: 'b', label: 'Bravo' },
    { id: 'c', label: 'Charlie', labelKey: 'charlieKey' }
  ]
}

function labels(): string[] {
  return Array.from(document.querySelectorAll('[data-testid="stack-item"]')).map((n) =>
    (n.textContent ?? '').replace(/[^A-Za-z]/g, '').slice(0, 5)
  )
}

describe('PosSensitiveList', () => {
  test('renders items in node order when value is missing', () => {
    render(<PosSensitiveList node={node} value={undefined} onChange={() => {}} />)
    expect(labels()).toEqual(['Alpha', 'Bravo', 'Charl'])
  })

  test('falls back to labelKey when item.labelKey is set', () => {
    render(<PosSensitiveList node={node} value={undefined} onChange={() => {}} />)
    // i18n mock returns the fallback (label) — but the key triggers the t() call.
    // The mock simply returns the fallback, so 'Charlie' still appears.
    expect(screen.getByText('Charlie')).toBeInTheDocument()
  })

  test('respects persisted positions and re-numbers from 1', () => {
    const value = {
      a: { pos: 3 },
      b: { pos: 1 },
      c: { pos: 2 }
    }
    render(<PosSensitiveList node={node} value={value} onChange={() => {}} />)
    expect(labels()).toEqual(['Bravo', 'Charl', 'Alpha'])
  })

  test('falls back to index-based position when pos is missing or non-finite', () => {
    const value = {
      a: { pos: Number.NaN }, // → idx+1 = 1
      b: {}, // → idx+1 = 2
      c: { pos: 0 } // explicit 0 wins → sorts first
    }
    render(<PosSensitiveList node={node} value={value} onChange={() => {}} />)
    expect(labels()[0]).toBe('Charl')
  })

  test('first row has the up button disabled', () => {
    render(<PosSensitiveList node={node} value={undefined} onChange={() => {}} />)
    const ups = screen.getAllByTestId('up-icon').map((u) => u.parentElement as HTMLButtonElement)
    expect(ups[0]).toBeDisabled()
  })

  test('last row has the down button disabled', () => {
    render(<PosSensitiveList node={node} value={undefined} onChange={() => {}} />)
    const downs = screen
      .getAllByTestId('down-icon')
      .map((d) => d.parentElement as HTMLButtonElement)
    expect(downs[downs.length - 1]).toBeDisabled()
  })

  test('clicking up swaps the row with its neighbour and stops event propagation', () => {
    const onChange = jest.fn()
    const onItemClick = jest.fn()
    render(
      <PosSensitiveList
        node={node}
        value={undefined}
        onChange={onChange}
        onItemClick={onItemClick}
      />
    )
    // Click the up arrow on the second row (Bravo)
    const ups = screen.getAllByTestId('up-icon').map((u) => u.parentElement as HTMLButtonElement)
    fireEvent.click(ups[1])

    expect(onChange).toHaveBeenCalledTimes(1)
    const next = onChange.mock.calls[0][0]
    expect(next.b.pos).toBe(1)
    expect(next.a.pos).toBe(2)
    // The StackItem click handler must not have fired
    expect(onItemClick).not.toHaveBeenCalled()
  })

  test('clicking down swaps with the next row', () => {
    const onChange = jest.fn()
    render(<PosSensitiveList node={node} value={undefined} onChange={onChange} />)
    const downs = screen
      .getAllByTestId('down-icon')
      .map((d) => d.parentElement as HTMLButtonElement)
    fireEvent.click(downs[0])
    const next = onChange.mock.calls[0][0]
    expect(next.a.pos).toBe(2)
    expect(next.b.pos).toBe(1)
  })

  test('shows the chevron only when onItemClick is provided', () => {
    const { rerender } = render(
      <PosSensitiveList node={node} value={undefined} onChange={() => {}} />
    )
    expect(screen.queryByTestId('chev-icon')).toBeNull()

    rerender(
      <PosSensitiveList node={node} value={undefined} onChange={() => {}} onItemClick={jest.fn()} />
    )
    expect(screen.getAllByTestId('chev-icon').length).toBe(node.items.length)
  })

  test('row click invokes onItemClick with route or id fallback', () => {
    const onItemClick = jest.fn()
    render(
      <PosSensitiveList
        node={node}
        value={undefined}
        onChange={() => {}}
        onItemClick={onItemClick}
      />
    )
    const rows = screen.getAllByTestId('stack-item')
    fireEvent.click(rows[0]) // Alpha → has route '/alpha'
    fireEvent.click(rows[1]) // Bravo → id 'b'
    expect(onItemClick).toHaveBeenNthCalledWith(1, '/alpha')
    expect(onItemClick).toHaveBeenNthCalledWith(2, 'b')
  })

  test('swap is a no-op for out-of-bounds indices (e.g. up on first row)', () => {
    const onChange = jest.fn()
    render(<PosSensitiveList node={node} value={undefined} onChange={onChange} />)
    // First row's up button is disabled, but assert no onChange even when fired programmatically
    const ups = screen.getAllByTestId('up-icon').map((u) => u.parentElement as HTMLButtonElement)
    // jsdom does fire onClick on disabled buttons via fireEvent — swap guards internally
    fireEvent.click(ups[0])
    expect(onChange).not.toHaveBeenCalled()
  })

  test('coerces a non-PosMap value (array) to an empty map', () => {
    render(<PosSensitiveList node={node} value={[1, 2, 3] as never} onChange={() => {}} />)
    expect(labels()).toEqual(['Alpha', 'Bravo', 'Charl'])
  })

  test('preserves untouched slot data when reordering', () => {
    const onChange = jest.fn()
    const value = {
      a: { pos: 1, extra: 'keep-me' },
      b: { pos: 2, extra: 'also' },
      c: { pos: 3 }
    }
    render(<PosSensitiveList node={node} value={value} onChange={onChange} />)
    const downs = screen
      .getAllByTestId('down-icon')
      .map((d) => d.parentElement as HTMLButtonElement)
    fireEvent.click(downs[0])
    const next = onChange.mock.calls[0][0]
    expect(next.a.extra).toBe('keep-me')
    expect(next.b.extra).toBe('also')
  })
})
