// Debounced JSON writer for the status-style files under userData.

import fs from 'node:fs'
import path from 'node:path'
import { app } from 'electron'

export type DebouncedJsonFileOpts = {
  debounceMs?: number
  /** Absolute path; without one it resolves to userData/<name> on first write. */
  file?: string
  /** Basename used when no explicit path is given. */
  name?: string
  tag?: string
}

export class DebouncedJsonFile {
  private flushTimer: NodeJS.Timeout | null = null
  private file: string | undefined
  private readonly name: string
  private readonly tag: string
  private readonly debounceMs: number

  constructor(
    private readonly snapshot: () => unknown,
    opts: DebouncedJsonFileOpts = {}
  ) {
    this.file = opts.file
    this.name = opts.name ?? 'data.json'
    this.tag = opts.tag ?? 'jsonFile'
    this.debounceMs = opts.debounceMs ?? 50
  }

  /** Queue a write, collapsing a burst into one. */
  schedule(): void {
    if (this.flushTimer) return
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null
      this.flushNow()
    }, this.debounceMs)
    this.flushTimer.unref?.()
  }

  /** Write now, skipping the debounce. */
  flushNow(): void {
    this.cancel()
    try {
      const file = this.target()
      fs.mkdirSync(path.dirname(file), { recursive: true })
      // tmp + rename so a reader gets either the old content or the new one
      const tmp = `${file}.tmp`
      fs.writeFileSync(tmp, `${JSON.stringify(this.snapshot(), null, 2)}\n`, 'utf8')
      fs.renameSync(tmp, file)
    } catch (e) {
      console.warn(`[${this.tag}] write failed:`, (e as Error).message)
    }
  }

  /** Drop a pending write. */
  cancel(): void {
    if (!this.flushTimer) return
    clearTimeout(this.flushTimer)
    this.flushTimer = null
  }

  private target(): string {
    this.file ??= path.join(app.getPath('userData'), this.name)
    return this.file
  }
}
