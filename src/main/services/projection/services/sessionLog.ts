import type { ProjectionSession, SessionDeviceIds } from './SessionManager'

function fmtKeys(d: SessionDeviceIds): string {
  const parts: string[] = []
  if (d.btMac) parts.push(`bt=${d.btMac}`)
  if (d.wifiMac) parts.push(`wifi=${d.wifiMac}`)
  if (d.usbUdid) parts.push(`udid=${d.usbUdid}`)
  if (d.instanceId) parts.push(`inst=${d.instanceId}`)
  if (d.controllerId) parts.push(`ctrl=${d.controllerId}`)
  if (d.ip) parts.push(`ip=${d.ip}`)
  return parts.length ? parts.join(' ') : '(none)'
}

export function logSessions(reason: string, sessions: readonly ProjectionSession[]): void {
  const bar = '═'.repeat(6)
  const head = `${bar} SESSIONS · ${reason} ${bar}`
  if (sessions.length === 0) {
    console.log(`${head}\n  (no sessions)`)
    return
  }
  const rows = sessions.map((s) => {
    const idx = `#${s.index}`.padEnd(3)
    const proto = s.protocol.padEnd(10)
    const trans = s.transport.padEnd(4)
    const state = (s.state === 'active' ? 'ACTIVE' : 'held').padEnd(6)
    const codec =
      `main:${s.video.main.codec ?? '-'} cluster:${s.video.cluster.codec ?? '-'}`.padEnd(24)
    return `  ${idx} ${proto} ${trans} ${state} ${codec} keys: ${fmtKeys(s.device)}`
  })
  console.log(`${head}\n${rows.join('\n')}`)
}
