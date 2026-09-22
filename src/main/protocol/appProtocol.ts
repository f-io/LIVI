import type { Config } from '@shared/types'
import { app, net, protocol } from 'electron'
import { existsSync, mkdirSync, writeFileSync } from 'fs'
import { homedir } from 'os'
import { join, normalize, sep } from 'path'
import { pathToFileURL } from 'url'
import { EXAMPLE_ICON } from '../services/custom/exampleIcon'
import { EXAMPLE_PAGE } from '../services/custom/examplePage'
import { themeCss } from '../services/custom/themeCss'

const CUSTOM_PREFIX = '/custom/'
const THEME_FILE = 'livi-theme.css'

let readConfig: () => Partial<Config> | undefined = () => undefined

/** The palette handed to the user's own page. */
export function setCustomPageConfig(getConfig: () => Partial<Config> | undefined): void {
  readConfig = getConfig
}

function customRoot(): string {
  return join(app.getPath('userData'), 'custom')
}

/** Written once */
export function seedCustomPage(): void {
  const root = customRoot()
  try {
    if (existsSync(root)) return
    mkdirSync(root, { recursive: true })
    const file = join(root, 'index.html')
    const home = homedir()
    const shown = file.startsWith(home) ? `~${file.slice(home.length)}` : file
    writeFileSync(file, EXAMPLE_PAGE.replace('{{FILE}}', shown))
    writeFileSync(join(root, 'icon.svg'), EXAMPLE_ICON)
    console.log(`[app-protocol] example page written to ${file}`)
  } catch (e) {
    console.warn(`[app-protocol] could not seed the custom folder: ${(e as Error).message}`)
  }
}

export const CUSTOM_PAGE_URL = 'app://index.html/custom/index.html'

export const CUSTOM_ICON_URL = 'app://index.html/custom/icon.svg'

/** True when the user's own page lies in the custom folder. */
export function customPageExists(): boolean {
  return existsSync(join(customRoot(), 'index.html'))
}

/** True when the custom folder names its own tab icon. */
export function customIconExists(): boolean {
  return existsSync(join(customRoot(), 'icon.svg'))
}

function customFile(path: string): string | null {
  const root = customRoot()
  const file = normalize(join(root, path.slice(CUSTOM_PREFIX.length - 1)))
  if (!file.startsWith(root + sep)) return null
  return existsSync(file) ? file : null
}

// protocol.registerSchemesAsPrivileged should be called before app is ready
// Protocol & Config
protocol.registerSchemesAsPrivileged([
  {
    scheme: 'app',
    privileges: {
      secure: true,
      standard: true,
      corsEnabled: true,
      supportFetchAPI: true,
      stream: true
    }
  }
])

export function registerAppProtocol() {
  protocol.handle('app', async (request) => {
    try {
      const u = new URL(request.url)
      let path = decodeURIComponent(u.pathname)
      if (path === '/' || path === '') path = '/index.html'

      if (path === `${CUSTOM_PREFIX}${THEME_FILE}`) {
        const cfg = readConfig()
        return new Response(themeCss(cfg, cfg?.darkMode !== false), {
          headers: { 'content-type': 'text/css; charset=utf-8', 'cache-control': 'no-store' }
        })
      }

      const file = path.startsWith(CUSTOM_PREFIX)
        ? customFile(path)
        : join(__dirname, '../renderer', path)
      if (!file || !existsSync(file)) {
        return new Response(null, { status: 404 })
      }

      const response = await net.fetch(pathToFileURL(file).toString())

      const headers = new Headers(response.headers)
      headers.set('Cross-Origin-Opener-Policy', 'same-origin')
      headers.set('Cross-Origin-Embedder-Policy', 'require-corp')
      headers.set('Cross-Origin-Resource-Policy', 'same-site')
      if (path.startsWith(CUSTOM_PREFIX)) headers.set('cache-control', 'no-store')

      return new Response(response.body, {
        status: response.status,
        headers
      })
    } catch (e) {
      console.error('[app-protocol] error', e)
      return new Response(null, { status: 500 })
    }
  })
}
