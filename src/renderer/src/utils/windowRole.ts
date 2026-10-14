export type WindowRole = 'main' | 'cluster' | 'aux'

export function getWindowRole(): WindowRole {
  try {
    const search = typeof window !== 'undefined' ? window.location.search : ''
    const role = new URLSearchParams(search).get('role')
    if (role === 'cluster' || role === 'aux') return role
    return 'main'
  } catch {
    return 'main'
  }
}
