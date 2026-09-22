import { cpSync, existsSync, rmSync } from 'node:fs'
import { builtinModules } from 'node:module'
import path, { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig, type Plugin } from 'vite'
import electron from 'vite-plugin-electron/simple'

const NODE_BUILTINS = [...builtinModules, ...builtinModules.map((m) => `node:${m}`)]
const BUILD_SHA = (process.env.GITHUB_SHA || process.env.BUILD_SHA || 'dev').slice(0, 7)
const BUILD_RUN = process.env.GITHUB_RUN_NUMBER || process.env.BUILD_RUN || ''
const BUILD_BRANCH = process.env.BUILD_BRANCH || ''

function copyAaResourcesPlugin(): Plugin {
  const aaRoot = resolve(import.meta.dirname, 'src/main/services/projection/driver/aa')
  const cpRoot = resolve(import.meta.dirname, 'src/main/services/projection/driver/cp')
  const protosSrc = path.join(aaRoot, 'protos')
  const cpIap2Src = path.join(cpRoot, 'iap2')
  const sharedSrc = resolve(import.meta.dirname, 'src/main/services/projection/driver/shared')
  const btSrc = resolve(import.meta.dirname, 'src/main/services/projection/driver/bt')
  const helperSrc = resolve(import.meta.dirname, 'src/main/services/projection/driver/helper')
  const protosDst = resolve(import.meta.dirname, 'out/main/protos')
  const driverDst = resolve(import.meta.dirname, 'out/main/driver')
  const btDst = resolve(import.meta.dirname, 'out/main/driver/bt')
  const cpIap2Dst = resolve(import.meta.dirname, 'out/main/driver/cp/iap2')
  const sharedDst = resolve(import.meta.dirname, 'out/main/driver/shared')
  const helperDst = resolve(import.meta.dirname, 'out/main/driver/helper')

  // Skip Python build droppings; the live process will recreate __pycache__.
  const filter = (src: string): boolean => !/[\\/]__pycache__([\\/]|$)/.test(src)

  let copied = false
  const copy = (): void => {
    if (copied) return
    copied = true
    if (existsSync(protosSrc)) {
      // Refresh: removing first guarantees deletions in the source propagate.
      try {
        rmSync(protosDst, { recursive: true, force: true })
      } catch {}
      cpSync(protosSrc, protosDst, { recursive: true, filter })
    }
    // Wipe the whole driver/ tree once (no stale files).
    try {
      rmSync(driverDst, { recursive: true, force: true })
    } catch {}
    const noTs = (src: string): boolean => filter(src) && !src.endsWith('.ts')
    if (existsSync(btSrc)) {
      cpSync(btSrc, btDst, { recursive: true, filter: noTs })
    }
    if (existsSync(helperSrc)) {
      cpSync(helperSrc, helperDst, { recursive: true, filter: noTs })
    }
    if (existsSync(sharedSrc)) {
      cpSync(sharedSrc, sharedDst, { recursive: true, filter })
    }
    if (existsSync(cpIap2Src)) {
      cpSync(cpIap2Src, cpIap2Dst, { recursive: true, filter })
    }
  }

  return {
    name: 'livi:copy-aa-resources',
    // Run on every main-process build, dev or prod.
    buildStart() {
      copied = false
    },
    closeBundle() {
      copy()
    }
  }
}

const mainAlias = {
  '@projection/messages': resolve(import.meta.dirname, 'src/main/services/projection/messages'),
  '@projection': resolve(import.meta.dirname, 'src/main/services/projection'),
  '@main': path.resolve(import.meta.dirname, 'src/main'),
  '@shared': path.resolve(import.meta.dirname, 'src/main/shared'),
  '@audio': path.resolve(import.meta.dirname, 'src/main/audio')
}

const rendererAlias = {
  '@pkg': resolve(import.meta.dirname, 'package.json'),
  '@settings': resolve(import.meta.dirname, 'src/renderer/src/components/pages/settings'),
  '@renderer': resolve(import.meta.dirname, 'src/renderer/src'),
  '@worker': path.resolve(import.meta.dirname, 'src/renderer/src/components/worker'),
  '@store': path.resolve(import.meta.dirname, 'src/renderer/src/store'),
  '@utils': path.resolve(import.meta.dirname, 'src/renderer/src/utils'),
  '@shared': path.resolve(import.meta.dirname, 'src/main/shared')
}

export default defineConfig({
  root: resolve(import.meta.dirname, 'src/renderer'),
  base: './',

  plugins: [
    react({}),
    electron({
      main: {
        entry: resolve(import.meta.dirname, 'src/main/index.ts'),
        onstart({ startup }) {
          startup(['.', '--no-sandbox'], { cwd: import.meta.dirname })
        },
        vite: {
          plugins: [copyAaResourcesPlugin()],
          resolve: {
            alias: mainAlias
          },
          build: {
            outDir: resolve(import.meta.dirname, 'out/main'),
            emptyOutDir: false,
            rolldownOptions: {
              external: [
                'electron',
                'usb',
                'livi-gst-video',
                'livi-crypto',
                'node-gyp-build',
                ...NODE_BUILTINS
              ],
              input: {
                main: resolve(import.meta.dirname, 'src/main/index.ts')
              },
              output: {
                format: 'cjs',
                entryFileNames: '[name].js'
              }
            }
          }
        }
      },

      preload: {
        input: resolve(import.meta.dirname, 'src/preload/index.ts'),
        vite: {
          resolve: {
            alias: mainAlias
          },
          build: {
            outDir: resolve(import.meta.dirname, 'out/preload'),
            emptyOutDir: false,
            rolldownOptions: {
              external: ['electron', ...NODE_BUILTINS],
              output: {
                format: 'cjs',
                entryFileNames: '[name].js'
              }
            }
          }
        }
      }
    })
  ],

  define: {
    __BUILD_SHA__: JSON.stringify(BUILD_SHA),
    __BUILD_RUN__: JSON.stringify(BUILD_RUN),
    __BUILD_BRANCH__: JSON.stringify(BUILD_BRANCH)
  },

  publicDir: resolve(import.meta.dirname, 'src/public'),

  build: {
    outDir: resolve(import.meta.dirname, 'out/renderer'),
    emptyOutDir: true,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        index: resolve(import.meta.dirname, 'src/renderer/index.html')
      },
      output: {
        entryFileNames: 'index.js',
        assetFileNames: (chunkInfo) => {
          const name = chunkInfo.name ?? ''
          if (name.endsWith('.css')) return 'index.css'
          if (/\.(woff2?|ttf|otf|eot)$/.test(name)) return '[name][extname]'
          return 'assets/[name][extname]'
        }
      }
    }
  },

  resolve: {
    alias: rendererAlias
  },

  server: {
    headers: {
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Resource-Policy': 'same-site'
    }
  },

  worker: {
    format: 'es'
  }
})
