import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

afterEach(() => {
  cleanup()
})

if (typeof globalThis.structuredClone === 'undefined') {
  ;(globalThis as typeof globalThis & { structuredClone: <T>(value: T) => T }).structuredClone = <
    T
  >(
    value: T
  ): T => JSON.parse(JSON.stringify(value)) as T
}

// jsdom 30.1 reports the Document as a focus event's relatedTarget; MUI's FocusTrap stores it and
// calls .focus() when it unmounts, which the Document has no method for.
if (typeof Document !== 'undefined' && typeof Document.prototype.focus !== 'function') {
  ;(Document.prototype as unknown as { focus: () => void }).focus = () => {}
}
