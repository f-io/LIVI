import '@testing-library/jest-dom'
import { TextDecoder, TextEncoder } from 'util'

if (typeof globalThis.TextEncoder === 'undefined') {
  ;(globalThis as typeof globalThis & { TextEncoder: typeof TextEncoder }).TextEncoder = TextEncoder
}

if (typeof globalThis.TextDecoder === 'undefined') {
  ;(globalThis as typeof globalThis & { TextDecoder: typeof TextDecoder }).TextDecoder = TextDecoder
}

if (typeof globalThis.structuredClone === 'undefined') {
  ;(
    globalThis as typeof globalThis & {
      structuredClone: <T>(value: T) => T
    }
  ).structuredClone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
}
