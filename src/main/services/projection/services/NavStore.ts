import type { NavLocale } from '@shared/utils'
import { translateNavigation } from '@shared/utils'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import type { NavigationData } from '../messages'
import { DEFAULT_NAVIGATION_DATA_RESPONSE } from './constants'
import type { ProjectionSession } from './SessionManager'
import type { PersistedNavigationPayload, ProjectionEvent } from './types'
import { normalizeNavigationPayload } from './utils/normalizeNavigation'

export type NavStoreDeps = {
  emit: (payload: ProjectionEvent) => void
  getLanguage: () => string | undefined
  isStarted: () => boolean
}

// Persists and emits the phone's navigation (turn-by-turn) snapshot, per session.
export class NavStore {
  private readonly pending = new Map<IPhoneDriver, PersistedNavigationPayload>()

  constructor(private readonly deps: NavStoreDeps) {}

  private file(): string {
    return path.join(app.getPath('userData'), 'navigationData.json')
  }

  private write(payload: PersistedNavigationPayload): void {
    const out = { timestamp: new Date().toISOString(), payload }
    fs.writeFileSync(this.file(), JSON.stringify(out, null, 2), 'utf8')
  }

  handle(
    driver: IPhoneDriver,
    session: ProjectionSession | null,
    msg: NavigationData,
    isActive: boolean
  ): void {
    if (!this.deps.isStarted()) return

    const existingPayload: PersistedNavigationPayload =
      session?.nav ?? this.pending.get(driver) ?? DEFAULT_NAVIGATION_DATA_RESPONSE.payload

    const language = this.deps.getLanguage()
    const locale: NavLocale =
      language === 'de'
        ? 'de'
        : language === 'ua' || language === 'uk' || language === 'uk-UA'
          ? 'ua'
          : 'en'

    const normalized = normalizeNavigationPayload(existingPayload, msg)
    const translated = translateNavigation(normalized.navi, locale)

    const nextPayload: PersistedNavigationPayload = {
      ...normalized,
      display: {
        locale,
        appName: translated.SourceName,
        destinationName: translated.DestinationName,
        roadName: translated.CurrentRoadName,
        maneuverText: translated.ManeuverTypeText,
        timeToDestinationText: translated.TimeRemainingToDestinationText,
        distanceToDestinationText: translated.DistanceRemainingDisplayStringText,
        remainDistanceText: translated.RemainDistanceText
      }
    }

    if (session) {
      session.nav = nextPayload
      this.pending.delete(driver)
      if (isActive) {
        this.deps.emit({ type: 'navigation', payload: msg })
        this.write(nextPayload)
      }
    } else {
      this.pending.set(driver, nextPayload)
    }
  }

  hydrate(session: ProjectionSession): void {
    try {
      this.write(session.nav ?? DEFAULT_NAVIGATION_DATA_RESPONSE.payload)
    } catch (e) {
      console.warn('[NavStore] hydrate failed (ignored)', e)
    }

    this.deps.emit({ type: 'navigation-reset', reason: 'session-switch' })
  }

  reset(reason: string): void {
    try {
      this.write(DEFAULT_NAVIGATION_DATA_RESPONSE.payload)
    } catch (e) {
      console.warn('[NavStore] reset failed (ignored)', reason, e)
    }

    this.deps.emit({ type: 'navigation-reset', reason })
  }
}
