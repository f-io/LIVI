import { net, protocol } from 'electron'
import { existsSync } from 'fs'
import { join } from 'path'
import { pathToFileURL } from 'url'

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

      const file = join(__dirname, '../renderer', path)
      if (!existsSync(file)) {
        return new Response(null, { status: 404 })
      }

      const response = await net.fetch(pathToFileURL(file).toString())

      const headers = new Headers(response.headers)
      headers.set('Cross-Origin-Opener-Policy', 'same-origin')
      headers.set('Cross-Origin-Embedder-Policy', 'require-corp')
      headers.set('Cross-Origin-Resource-Policy', 'same-site')

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
