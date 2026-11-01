import type { TelemetryPayload } from '@shared/types/Telemetry'

export function decodeHaltechCanLine(line: string): TelemetryPayload | null {
  const match = /^\s*(?:\([^)]*\)\s+)?can0\s+([0-9a-f]{1,3})#([0-9a-f]{0,16})\s*$/i.exec(line)
  if (!match || match[2].length % 2 !== 0) return null

  const id = Number.parseInt(match[1], 16)
  if (id > 0x7ff) return null

  const data = match[2].match(/.{2}/g)?.map((byte) => Number.parseInt(byte, 16)) ?? []
  if (data.length < 2) return null

  const raw = (data[0] << 8) | data[1]
  if (id === 0x360) return { rpm: raw }
  if (id === 0x370) return { speedKph: raw / 10 }
  return null
}
