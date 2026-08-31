// Builds the native cargo targets and places the artifacts where their loaders
// expect them:
//   livi-crypto-node  -> native/livi-crypto/build/Release/livi_crypto.node
//   gst-video-addon   -> native/livi-gst-video/build/Release/gst_video.node
//   gst-video-host    -> native/livi-gst-video/build/Release/livi-gst-host (linux)
//   livi-compositor   -> out/compositor/livi-compositor (linux)
//
// Usage: node scripts/build-native.mjs [--arch=x64|arm64] [--only=crypto]
// Linux runners are arch-native; only macOS cross-compiles (arm64 host -> x64 app).
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const archArg = process.argv.find((a) => a.startsWith('--arch='))?.slice(7)
const only = process.argv.find((a) => a.startsWith('--only='))?.slice(7)
const wantArch = archArg === 'x64' ? 'x64' : archArg === 'arm64' ? 'arm64' : process.arch
const cross = process.platform === 'darwin' && wantArch !== process.arch
const triple = wantArch === 'x64' ? 'x86_64-apple-darwin' : 'aarch64-apple-darwin'

const dylib = (name) =>
  process.platform === 'darwin' ? `lib${name}.dylib` : `lib${name}.so`

// macOS keeps GStreamer in a framework, so pkg-config has to be pointed at it.
const GST_FRAMEWORK_PC = '/Library/Frameworks/GStreamer.framework/Versions/1.0/lib/pkgconfig'

function cargoEnv() {
  const env = { ...process.env, PKG_CONFIG_ALLOW_CROSS: '1' }
  if (process.platform === 'darwin') {
    env.PKG_CONFIG_PATH = [GST_FRAMEWORK_PC, process.env.PKG_CONFIG_PATH]
      .filter(Boolean)
      .join(':')
  }
  return env
}

function cargoBuild(manifest, pkg) {
  const args = ['build', '--release', '-p', pkg, '--manifest-path', manifest]
  if (cross) {
    execFileSync('rustup', ['target', 'add', triple], { stdio: 'inherit' })
    args.push('--target', triple)
  }
  execFileSync('cargo', args, { stdio: 'inherit', env: cargoEnv() })
  return join(dirname(manifest), 'target', ...(cross ? [triple] : []), 'release')
}

function place(src, destDir, destName) {
  mkdirSync(destDir, { recursive: true })
  copyFileSync(src, join(destDir, destName))
  console.log(`[build-native] ${destName} <- ${src}`)
}

const cryptoOut = cargoBuild(join(root, 'native', 'livi-crypto', 'rust', 'Cargo.toml'), 'livi-crypto-node')
place(
  join(cryptoOut, dylib('livi_crypto_node')),
  join(root, 'native', 'livi-crypto', 'build', 'Release'),
  'livi_crypto.node'
)

if (only !== 'crypto') {
  const gstManifest = join(root, 'native', 'livi-gst-video', 'rust', 'Cargo.toml')
  const gstDest = join(root, 'native', 'livi-gst-video', 'build', 'Release')
  const addonOut = cargoBuild(gstManifest, 'gst-video-addon')
  place(join(addonOut, dylib('gst_video_addon')), gstDest, 'gst_video.node')
  if (process.platform === 'linux') {
    const hostOut = cargoBuild(gstManifest, 'gst-video-host')
    place(join(hostOut, 'livi-gst-host'), gstDest, 'livi-gst-host')

    const compManifest = join(root, 'native', 'livi-compositor', 'rust', 'Cargo.toml')
    const compOut = cargoBuild(compManifest, 'livi-compositor')
    place(join(compOut, 'livi-compositor'), join(root, 'out', 'compositor'), 'livi-compositor')
  }
}
