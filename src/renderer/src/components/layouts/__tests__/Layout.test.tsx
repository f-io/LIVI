import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { Layout } from '../Layout'

describe('Layout', () => {
  test('renders outlet content inside main-root container', () => {
    const { container } = render(
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<div>Child content</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    )

    expect(screen.getByText('Child content')).toBeInTheDocument()
    expect(container.querySelector('#main-root')).toBeTruthy()
  })
})
