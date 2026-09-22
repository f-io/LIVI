import type { Config } from '@shared/types'

// Live binding: importers see runtime changes. The DEBUG env var always wins as a floor.
export let DEBUG = process.env.DEBUG === '1'
export const TRACE = process.env.TRACE === '1'

export function setDebugLogging(enabled: boolean): void {
  DEBUG = enabled || process.env.DEBUG === '1'
}

export const MIN_WIDTH = 300
export const MIN_HEIGHT = 200
export const MAX_WIDTH = 4096
export const MAX_HEIGHT = 2160
export const DEFAULT_WIDTH = 800
export const DEFAULT_HEIGHT = 480

export const NULL_DELETES: (keyof Config)[] = [
  'primaryColorDark',
  'primaryColorLight',
  'highlightColorDark',
  'highlightColorLight'
  // add more explicit “reset-to-default” keys here
]

export const mimeTypeFromExt = (ext: string): string =>
  (
    ({
      '.html': 'text/html',
      '.js': 'text/javascript',
      '.css': 'text/css',
      '.png': 'image/png',
      '.jpg': 'image/jpeg',
      '.jpeg': 'image/jpeg',
      '.svg': 'image/svg+xml',
      '.json': 'application/json',
      '.wasm': 'application/wasm',
      '.map': 'application/json'
    }) as const
  )[ext.toLowerCase()] ?? 'application/octet-stream'
