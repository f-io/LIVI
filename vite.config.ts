import path, { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import electron from 'vite-plugin-electron/simple'

const BUILD_SHA = (process.env.GITHUB_SHA || process.env.BUILD_SHA || 'dev').slice(0, 7)
const BUILD_RUN = process.env.GITHUB_RUN_NUMBER || process.env.BUILD_RUN || ''
const BUILD_BRANCH = process.env.BUILD_BRANCH || ''

const mainAlias = {
  '@projection/web': resolve(__dirname, 'src/renderer/src/components/web/CarplayWeb.ts'),
  '@projection/messages': resolve(__dirname, 'src/main/services/projection/messages'),
  '@projection': resolve(__dirname, 'src/main/services/projection'),
  '@main': path.resolve(__dirname, 'src/main'),
  '@shared': path.resolve(__dirname, 'src/main/shared'),
  '@audio': path.resolve(__dirname, 'src/main/audio')
}

const rendererAlias = {
  '@renderer': resolve(__dirname, 'src/renderer/src'),
  '@worker': path.resolve(__dirname, 'src/renderer/src/components/worker'),
  '@store': path.resolve(__dirname, 'src/renderer/src/store'),
  '@utils': path.resolve(__dirname, 'src/renderer/src/utils'),
  '@shared': path.resolve(__dirname, 'src/main/shared')
}

export default defineConfig({
  root: resolve(__dirname, 'src/renderer'),
  base: './',

  plugins: [
    react({}),
    electron({
      main: {
        entry: 'src/main/index.ts',
        vite: {
          resolve: {
            alias: mainAlias
          },
          build: {
            outDir: resolve(__dirname, 'out/main'),
            emptyOutDir: false,
            rollupOptions: {
              external: ['electron', 'usb', 'node-gyp-build'],
              input: {
                main: resolve(__dirname, 'src/main/index.ts'),
                usbWorker: resolve(__dirname, 'src/main/services/usb/USBWorker.ts')
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
        input: resolve(__dirname, 'src/preload/index.ts'),
        vite: {
          resolve: {
            alias: mainAlias
          },
          build: {
            outDir: resolve(__dirname, 'out/preload'),
            emptyOutDir: false,
            rollupOptions: {
              external: ['electron'],
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

  publicDir: resolve(__dirname, 'src/public'),

  build: {
    outDir: resolve(__dirname, 'out/renderer'),
    emptyOutDir: true,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        index: resolve(__dirname, 'src/renderer/index.html')
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
