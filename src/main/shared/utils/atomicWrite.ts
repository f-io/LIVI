import { promises as fsp, renameSync, writeFileSync } from 'node:fs'

/**
 * Writes `data` into a neighbouring `.tmp` and renames it over `file`. The
 * rename is one step, so a reader gets either the old content or the new one,
 * and a machine that goes down mid-write keeps the old one.
 */
export function writeFileAtomic(file: string, data: string, mode?: number): void {
  const tmp = `${file}.tmp`
  if (mode === undefined) {
    writeFileSync(tmp, data)
  } else {
    writeFileSync(tmp, data, { mode })
  }
  renameSync(tmp, file)
}

/** `writeFileAtomic` for callers that already work off the event loop. */
export async function writeFileAtomicAsync(
  file: string,
  data: string,
  mode?: number
): Promise<void> {
  const tmp = `${file}.tmp`
  if (mode === undefined) {
    await fsp.writeFile(tmp, data)
  } else {
    await fsp.writeFile(tmp, data, { mode })
  }
  await fsp.rename(tmp, file)
}
