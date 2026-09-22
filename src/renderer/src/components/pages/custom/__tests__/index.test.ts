import * as custom from '..'

test('re-exports the page', () => {
  expect(typeof custom.Custom).toBe('function')
})
