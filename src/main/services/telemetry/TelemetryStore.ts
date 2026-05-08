/**
 * Central merge store for incoming telemetry.
 *
 *   ingest.ts ── merge(patch) ──► TelemetryStore ── 'change' ──► adapters
 *
 */

import { EventEmitter } from 'node:events'
import type { TelemetryPayload } from '@shared/types/Telemetry'

const NESTED_KEYS = new Set(['gps', 'can'])

export interface TelemetryStoreEvents {
  change: (patch: TelemetryPayload, snapshot: TelemetryPayload) => void
}

export class TelemetryStore extends EventEmitter {
  private _snapshot: TelemetryPayload = {}

  /** Read-only view of the merged state. Returns a fresh shallow copy. */
  snapshot(): TelemetryPayload {
    return { ...this._snapshot }
  }

  /** Merge a partial payload and notify listeners. Empty patches are no-ops. */
  merge(patch: TelemetryPayload | null | undefined): void {
    if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return
    if (Object.keys(patch).length === 0) return

    // Build the merged snapshot. Nested known sub-blocks merge field-by-field;
    // everything else is replace-on-write.
    const next: TelemetryPayload = { ...this._snapshot }
    for (const key of Object.keys(patch)) {
      const incoming = (patch as Record<string, unknown>)[key]
      if (incoming === undefined) continue
      if (NESTED_KEYS.has(key) && _isPlainObject(incoming)) {
        const prev = (this._snapshot as Record<string, unknown>)[key]
        ;(next as Record<string, unknown>)[key] = _isPlainObject(prev)
          ? { ...prev, ...incoming }
          : { ...incoming }
      } else {
        ;(next as Record<string, unknown>)[key] = incoming
      }
    }

    if (next.ts === undefined) next.ts = Date.now()
    this._snapshot = next

    // Build the patch we re-emit. For nested keys we re-emit only the merged
    // sub-block (so receivers don't need to know about merge semantics).
    const emittedPatch: TelemetryPayload = {}
    for (const key of Object.keys(patch)) {
      const incoming = (patch as Record<string, unknown>)[key]
      if (incoming === undefined) continue
      if (NESTED_KEYS.has(key)) {
        ;(emittedPatch as Record<string, unknown>)[key] = (next as Record<string, unknown>)[key]
      } else {
        ;(emittedPatch as Record<string, unknown>)[key] = incoming
      }
    }
    if (emittedPatch.ts === undefined) emittedPatch.ts = next.ts

    this.emit('change', emittedPatch, { ...this._snapshot })
  }

  /** Reset the store. Mostly for tests / explicit teardown. */
  reset(): void {
    this._snapshot = {}
  }

  // Strongly-typed event helpers
  override on<K extends keyof TelemetryStoreEvents>(
    event: K,
    listener: TelemetryStoreEvents[K]
  ): this {
    return super.on(event, listener as (...args: unknown[]) => void)
  }
  override off<K extends keyof TelemetryStoreEvents>(
    event: K,
    listener: TelemetryStoreEvents[K]
  ): this {
    return super.off(event, listener as (...args: unknown[]) => void)
  }
}

function _isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}
