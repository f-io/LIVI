import { ExtraConfig } from './Globals'

export const MIN_WIDTH = 400
export const DEFAULT_WIDTH = 800
export const DEFAULT_HEIGHT = 480

export const NULL_DELETES: (keyof ExtraConfig)[] = [
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
