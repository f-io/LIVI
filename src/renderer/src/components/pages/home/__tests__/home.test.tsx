import { render, screen, fireEvent } from '@testing-library/react'

// TODO Test file.

function Button({ onClick }: { onClick: () => void }) {
  return <button onClick={onClick}>Click me</button>
}

test('Button calls onClick when pressed', () => {
  const handleClick = jest.fn()
  render(<Button onClick={handleClick} />)
  fireEvent.click(screen.getByText('Click me'))
  expect(handleClick).toHaveBeenCalledTimes(1)
})
