import { lazy } from 'react'

export const FFTSpectrum = lazy(() =>
  import('./FFTSpectrum').then((m) => ({ default: m.FFTSpectrum }))
)
