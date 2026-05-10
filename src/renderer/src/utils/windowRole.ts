export type WindowRole = 'main' | 'dash' | 'aux'

export function getWindowRole(): WindowRole {
  try {
    const search = typeof window !== 'undefined' ? window.location.search : ''
    const role = new URLSearchParams(search).get('role')
    if (role === 'dash' || role === 'aux') return role
    return 'main'
  } catch {
    return 'main'
  }
}
