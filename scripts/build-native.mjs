// Usage: node scripts/build-native.mjs [--arch=x64|arm64]
// Linux runners are arch-native, only macOS cross-compiles (arm64 host -> x64 app).
import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const archArg = process.argv.find((a) => a.startsWith('--arch='))?.slice(7)
const wantArch = archArg === 'x64' ? 'x64' : archArg === 'arm64' ? 'arm64' : process.arch
const cross = process.platform === 'darwin' && wantArch !== process.arch
const triple = wantArch === 'x64' ? 'x86_64-apple-darwin' : 'aarch64-apple-darwin'

// gst-host builds against the GStreamer framework's headers on macOS, the app
// ships its own bundle of the libraries.
const MAC_GST_PKGCONFIG = '/Library/Frameworks/GStreamer.framework/Versions/1.0/lib/pkgconfig'

function cargoEnv() {
  const env = { ...process.env, PKG_CONFIG_ALLOW_CROSS: '1' }
  if (process.platform === 'darwin' && existsSync(MAC_GST_PKGCONFIG)) {
    env.PKG_CONFIG_PATH = [MAC_GST_PKGCONFIG, env.PKG_CONFIG_PATH].filter(Boolean).join(':')
  }
  return env
}

const manifest = join(root, 'native', 'Cargo.toml')

function cargoBuild(pkgs) {
  const args = ['build', '--release', '--manifest-path', manifest, ...pkgs.flatMap((p) => ['-p', p])]
  // A manifest edit without its lockfile fails in CI instead of resolving anew.
  if (process.env.CI) args.push('--locked')
  if (cross) {
    execFileSync('rustup', ['target', 'add', triple], { stdio: 'inherit' })
    args.push('--target', triple)
  }
  execFileSync('cargo', args, { stdio: 'inherit', env: cargoEnv() })
  return join(targetDir(), ...(cross ? [triple] : []), 'release')
}

// CARGO_TARGET_DIR or a target-dir in the cargo config may move the build out of the tree.
function targetDir() {
  const meta = execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifest],
    { encoding: 'utf8', env: cargoEnv(), maxBuffer: 16 * 1024 * 1024 }
  )
  return JSON.parse(meta).target_directory
}

function place(src, destDir, destName) {
  mkdirSync(destDir, { recursive: true })
  copyFileSync(src, join(destDir, destName))
  console.log(`[build-native] ${destName} <- ${src}`)
}

const pkgs = ['gst-video-host', 'livi-helperd', 'livi-core']
if (process.platform === 'darwin') pkgs.push('gst-video-addon')
if (process.platform === 'linux') pkgs.push('livi-compositor')
const out = cargoBuild(pkgs)

const gstDest = join(root, 'native', 'livi-gst-video', 'build', 'Release')
place(join(out, 'livi-gst-host'), gstDest, 'livi-gst-host')
if (process.platform === 'darwin') {
  place(join(out, 'libgst_video_addon.dylib'), gstDest, 'gst_video.node')
}
if (process.platform === 'linux') {
  place(join(out, 'livi-compositor'), join(root, 'out', 'compositor'), 'livi-compositor')
}

const helperDest = join(root, 'native', 'livi-helperd', 'build', 'Release')
place(join(out, 'livi-helperd'), helperDest, 'livi-helperd')
place(join(out, 'livi-core'), helperDest, 'livi-core')
