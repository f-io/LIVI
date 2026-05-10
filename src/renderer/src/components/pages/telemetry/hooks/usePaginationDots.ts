import { useCallback } from 'react'

export const usePaginationDots = (_isNavbarHidden: boolean) => {
  return {
    showDots: true,
    revealDots: useCallback(() => {}, [])
  }
}
