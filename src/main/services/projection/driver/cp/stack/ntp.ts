const NTP_EPOCH_OFFSET = 2208988800

/** Now as a 64-bit NTP timestamp, in the clock domain TimingSync uses. */
export function ntp64Now(): bigint {
  const t = Date.now() / 1000 + NTP_EPOCH_OFFSET
  const sec = Math.floor(t)
  const frac = Math.floor((t - sec) * 0x100000000)
  return (BigInt(sec) << 32n) | BigInt(frac >>> 0)
}
