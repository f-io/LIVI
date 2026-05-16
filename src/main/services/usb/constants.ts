//Carlinkit dongle identification at runtime

export const CARLINKIT_VID = 0x1314
export const CARLINKIT_PIDS = [0x1520, 0x1521] as const

export function isCarlinkitDongle(vid: number | undefined, pid: number | undefined): boolean {
  if (vid !== CARLINKIT_VID) return false
  return (CARLINKIT_PIDS as readonly number[]).includes(pid ?? -1)
}
